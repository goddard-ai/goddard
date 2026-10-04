import { useRef } from 'octane';
import { popRoute, pushRoute } from '@octane-xplat/ui';

import { QrScanner } from '../components/qr-scanner';
import { showAlert } from '../lib/alert';
import { parseDaemonConnectLink } from '../lib/daemon-profile';

/** QR scanner for `goddard://connect` codes. The platform leaf owns the
 * camera (native) or the manual-entry fallback (web); a parsed code routes
 * through the shared connect screen, which saves and activates the profile. */
export default function DaemonScanScreen() {
  const handled = useRef(false);

  const onCode = (raw: string) => {
    if (handled.current) {return;}
    try {
      const link = parseDaemonConnectLink(raw);
      handled.current = true;
      // Pop the scanner first so connect's own pop lands back on the
      // caller instead of a live camera.
      popRoute('root');
      pushRoute({ stack: 'root', name: 'connect', params: { ...link } });
    } catch {
      void showAlert(
        'Not a pairing code',
        'That QR code isn’t a Goddard connect code. Scan the code shown in Settings → Daemon on your host.',
      );
    }
  };

  return (
    <QrScanner
      onCode={onCode}
      onManual={() => pushRoute({ stack: 'root', name: 'daemon-editor', params: {} })}
    />
  );
}
