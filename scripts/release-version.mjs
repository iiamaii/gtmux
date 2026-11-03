// CI-only version stamping. No git mutations, tags or releases are created.
import { readFile, writeFile } from 'node:fs/promises';
const tag = process.env.GTMUX_RELEASE_TAG;
if (tag) {
  const version = tag.replace(/^v/, '');
  if (!/^\d+\.\d+\.\d+(?:-[0-9A-Za-z.-]+)?$/.test(version)) throw new Error('Release tags must contain a semantic version.');
  for (const file of ['codebase/launcher/package.json','codebase/launcher/package-lock.json']) {
    const doc = JSON.parse(await readFile(file,'utf8')); doc.version = version;
    if (doc.packages?.['']) doc.packages[''].version = version;
    await writeFile(file,JSON.stringify(doc,null,2)+'\n');
  }
}
