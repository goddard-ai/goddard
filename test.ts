import { command, run, string, number, positional, option, subcommands, flag, restPositionals, binary, runSafely } from 'cmd-ts';

const app = subcommands({
  name: 'goddard',
  cmds: {
    login: command({
      name: 'login',
      args: { username: option({ type: string, long: 'username' }) },
      handler: () => {}
    })
  }
});

runSafely(app, ['node', 'goddard', 'unknown']).then(res => console.log('res:', res)).catch(e => {
  console.log('caught error', e.name);
});
