import fs from 'node:fs/promises';

export async function load(url, context, nextLoad) {
  if (url.endsWith('?raw')) {
    const filePath = new URL(url).pathname.replace(/\?raw$/, '');
    const source = await fs.readFile(filePath, 'utf8');
    return {
      format: 'module',
      source: `export default ${JSON.stringify(source)};`,
      shortCircuit: true,
    };
  }
  return nextLoad(url, context);
}
