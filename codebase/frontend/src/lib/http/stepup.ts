// Step-up re-auth shared error types (ADR-0020 D16).
//
// `POST /api/shutdown` and `POST /auth/rotate` require an inline credential
// (mode-aware password | token) re-verified by the backend *before* the action
// runs. Their failure modes overlap, so the typed errors live here and both
// HTTP clients throw them. ReauthModal branches on these to stay open + show an
// inline message vs. closing.
//
// IMPORTANT — distinct from `UnauthorizedError`:
//   A step-up 401 (`invalid_credential` / `credential_required`) means *wrong
//   credential, retry* — the modal must stay open and NOT redirect to /auth.
//   A genuine session-expiry 401 (no step-up error body) still surfaces as
//   `UnauthorizedError` so the caller redirects. The BE error body string
//   discriminates the two.

/** Wrong password / token. Keep the modal open and let the user retry. */
export class InvalidCredentialError extends Error {
  constructor(message = 'Incorrect credential.') {
    super(message);
    this.name = 'InvalidCredentialError';
  }
}

/** Empty / missing credential in the request body. */
export class CredentialRequiredError extends Error {
  constructor(message = 'A credential is required.') {
    super(message);
    this.name = 'CredentialRequiredError';
  }
}

/** Rate limited (password mode, D5 limiter). 429 + Retry-After. */
export class RateLimitedError extends Error {
  /** Seconds the caller should wait before retrying, when the BE reports it. */
  readonly retryAfterSecs: number | null;
  constructor(retryAfterSecs: number | null, message = 'Too many attempts.') {
    super(message);
    this.name = 'RateLimitedError';
    this.retryAfterSecs = retryAfterSecs;
  }
}

/** Parse a `Retry-After` header (delta-seconds form) into a number, or null. */
export function parseRetryAfter(res: Response): number | null {
  const raw = res.headers.get('Retry-After');
  if (raw === null) return null;
  const secs = Number.parseInt(raw, 10);
  return Number.isFinite(secs) ? secs : null;
}

/**
 * Map a shared shutdown/rotate step-up error response to a typed error.
 * Returns the error to throw, or `null` if this status/body is NOT a step-up
 * failure (the caller then handles it as a generic / session error).
 *
 * The response body is consumed here only when a step-up error is recognised.
 */
export async function stepUpErrorFor(res: Response): Promise<Error | null> {
  if (res.status === 429) {
    return new RateLimitedError(
      parseRetryAfter(res),
      'Too many attempts. Try again later.',
    );
  }
  if (res.status === 401) {
    const body = await res
      .json()
      .catch(() => ({}) as { error?: string });
    const code = (body as { error?: string }).error;
    if (code === 'invalid_credential') {
      return new InvalidCredentialError();
    }
    if (code === 'credential_required') {
      return new CredentialRequiredError();
    }
    // 401 without a step-up error code → genuine session expiry; let the
    // caller raise UnauthorizedError (redirect path).
    return null;
  }
  return null;
}
