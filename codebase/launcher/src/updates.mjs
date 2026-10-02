import { join } from 'node:path';
import { atomicWrite, readOptional } from './files.mjs';
export class Updates {
  constructor({ root, updater, supported = true, reason = '', stopForInstall, installFailed = () => {} }) {
    this.installFailed = installFailed;
    this.path = join(root, 'updates.json'); this.updater = updater; this.supported = supported;
    this.stopForInstall = stopForInstall; this.automatic = false; this.phase = 'idle'; this.error = reason; this.version = null; this.progress = 0; this.busy = false;
    if (updater) {
      updater.autoDownload = false; updater.autoInstallOnAppQuit = false;
      updater.allowPrerelease = false; updater.allowDowngrade = false;
      updater.on('update-available', () => { this.available = true; });
      updater.on('update-not-available', () => { this.available = false; });
      updater.on('download-progress', p => { this.progress = Math.max(0, Math.min(100, p.percent)); });
      updater.on('error', error => { if (this.phase === 'installing') this.installFailed(); this.error = error.message; this.phase = 'error'; });
    }
  }
  status() { return { supported: this.supported, automatic: this.automatic, phase: this.phase, version: this.version, progress: this.progress, error: this.error }; }
  async load() { const value = JSON.parse(await readOptional(this.path) ?? '{}'); this.automatic = value.automatic === true; return this.status(); }
  async preferences({ automatic }) {
    if (typeof automatic !== 'boolean') throw new Error('Automatic update preference must be a boolean.');
    if (!this.supported && automatic) throw new Error(this.error || 'Updates are unavailable for this package.');
    await atomicWrite(this.path, JSON.stringify({ automatic })); this.automatic = automatic; return this.status();
  }
  async check(download = false) {
    if (!this.supported) throw new Error(this.error || 'Updates are unavailable for this package.');
    if (this.busy || this.phase === 'ready') return this.status();
    this.busy = true; this.error = ''; this.available = false; this.phase = 'checking';
    try {
      const result = await this.updater.checkForUpdates();
      // Never infer availability just because the endpoint returned metadata.
      if (!result || !this.available) { this.phase = 'current'; this.version = null; return this.status(); }
      this.version = result.updateInfo.version; this.phase = 'available';
      if (download || this.automatic) {
        this.phase = 'downloading'; this.progress = 0;
        await this.updater.downloadUpdate(); this.phase = 'ready'; this.progress = 100;
      }
      return this.status();
    } catch (e) { this.phase = 'error'; this.error = e.message; throw e; }
    finally { this.busy = false; }
  }
  async install({ confirmed, credential }) {
    if (!confirmed) throw new Error('Confirm that installation will stop running terminal programs and restart the app.');
    if (this.busy || this.phase !== 'ready') throw new Error('Download and verify an update before installing.');
    this.busy = true;
    try { await this.stopForInstall(credential); this.phase = 'installing'; this.updater.quitAndInstall(false, true); }
    catch (e) { if (this.phase === 'installing') { this.phase = 'error'; this.installFailed(); } this.error = e.message; throw e; }
    finally { this.busy = false; }
    return this.status();
  }
  startSchedule() {
    const tick = () => { if (this.automatic && this.supported) void this.check(true).catch(() => {}); };
    this.timer = setInterval(tick, 6 * 60 * 60 * 1000); this.timer.unref?.(); tick();
  }
  close() { clearInterval(this.timer); }
}
