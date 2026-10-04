import xtermCss from '@xterm/xterm/css/xterm.css?raw';
import xtermJs from '@xterm/xterm/lib/xterm.js?raw';
import fitAddonJs from '@xterm/addon-fit/lib/addon-fit.js?raw';

export interface XtermAssets {
  css: string;
  js: string;
  fit: string;
}

export const xtermAssets: XtermAssets | null = {
  css: xtermCss,
  js: xtermJs,
  fit: fitAddonJs,
};
