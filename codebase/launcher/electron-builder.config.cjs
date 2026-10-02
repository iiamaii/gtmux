const config = require('./package.json').build;
const repository = process.env.GTMUX_RELEASE_REPOSITORY;
if (repository && !/^[a-zA-Z0-9_.-]+\/[a-zA-Z0-9_.-]+$/.test(repository)) throw new Error('Invalid release repository.');
module.exports = { ...config, publish: repository ? { provider: 'github', owner: repository.split('/')[0], repo: repository.split('/')[1], releaseType: 'release' } : null };
