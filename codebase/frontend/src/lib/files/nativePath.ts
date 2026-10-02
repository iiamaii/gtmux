/** Normalize Windows paths for UI joins without rewriting POSIX backslash names.
 * The server still canonicalizes and authorizes every filesystem request. */
export function normalizeNativePath(path: string): string {
  if (path.startsWith('\\\\?\\UNC\\')) return '//' + path.slice(8).replace(/\\/g, '/');
  if (path.startsWith('\\\\?\\')) return path.slice(4).replace(/\\/g, '/');
  if (/^[a-z]:[\\/]/i.test(path) || path.startsWith('\\\\')) return path.replace(/\\/g, '/');
  return path;
}
export function isAbsoluteNativePath(path: string): boolean {
  const value = normalizeNativePath(path);
  return value.startsWith('/') || /^[a-z]:\//i.test(value);
}
