import assert from 'node:assert/strict';
import { execFile } from 'node:child_process';
import { promisify } from 'node:util';
import { setTimeout as delay } from 'node:timers/promises';

const exec = promisify(execFile);
const udid = process.argv[2];
assert(udid, 'Usage: pnpm smoke:ios SIMULATOR_UDID (requires a running, unpaired test app and idb)');

async function idb(...args) {
  const { stdout } = await exec('idb', [...args, '--udid', udid], {
    timeout: 30_000,
    maxBuffer: 8 * 1024 * 1024,
  });
  return stdout;
}

function elements(value, result = []) {
  if (Array.isArray(value)) {
    for (const child of value) {elements(child, result);}
  } else if (value && typeof value === 'object') {
    if (value.type && value.frame) {result.push(value);}
    for (const [key, child] of Object.entries(value)) {
      if (key !== 'AXValue' && key !== 'value') {elements(child, result);}
    }
  }
  return result;
}

const snapshot = async () => elements(JSON.parse(await idb('ui', 'describe-all', '--api', 'axbridge', '--json')));
const named = (state, label) => state.find((element) => element.AXLabel === label);
const fields = (state) => state.filter((element) => element.type === 'TextField').sort((a, b) => a.frame.y - b.frame.y);

async function until(check, description) {
  const deadline = Date.now() + 20_000;
  do {
    const state = await snapshot();
    if (check(state)) {return state;}
    await delay(250);
  } while (Date.now() < deadline);
  throw new Error(`Timed out: ${description}`);
}

async function tap(element) {
  assert(element?.frame, 'Expected an accessibility element with a frame');
  const { x, y, width, height } = element.frame;
  assert(width > 0 && height > 0, 'Cannot tap an empty accessibility frame');
  await idb('ui', 'tap', String(Math.round(x + width / 2)), String(Math.round(y + height / 2)));
}

async function enter(index, text) {
  await tap(fields(await snapshot())[index]);
  await idb('ui', 'key', '4', '--command');
  await idb('ui', 'text', text);
  await until((state) => fields(state)[index]?.AXValue === text, 'text input did not retain the test value');
}

const initial = await snapshot();
assert(named(initial, 'Add a daemon'), 'Use a disposable, unpaired simulator app at onboarding; existing connections are left untouched');
await tap(named(initial, 'Add a daemon'));
await until((state) => named(state, 'Add Daemon') && fields(state).length === 2, 'daemon editor did not open');
console.log('PASS: first tap opens the daemon editor without crashing');

const name = 'iOS smoke test';
const address = 'ws://127.0.0.1:34123';
await enter(0, name);
await enter(1, 'not a valid host');
await until((state) => named(state, 'Enter a valid ws:// or wss:// WebSocket address.'), 'invalid address feedback is missing');
console.log('PASS: invalid addresses are rejected');

await enter(1, address);
await until((state) => named(state, 'Unencrypted connection. Use it only on a LAN or tailnet you trust.'), 'LAN address feedback is missing');
const beforeDismiss = await snapshot();
const editorY = named(beforeDismiss, 'Add Daemon').frame.y;
await tap(named(beforeDismiss, 'Add Daemon'));
await until((state) => {
  const editor = named(state, 'Add Daemon');
  return editor && editor.frame.y > editorY + 100
    && !state.some((element) => element.type === 'Keyboard')
    && fields(state)[0]?.AXValue === name
    && fields(state)[1]?.AXValue === address;
}, 'keyboard did not dismiss with input preserved');
console.log('PASS: keyboard dismissal preserves the daemon name and address');
console.log('No credentials entered, profiles saved, or prompts submitted. Authenticated workflows require a separate check.');
