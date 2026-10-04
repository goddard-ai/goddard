import { ScanFallback } from './scan-fallback';
import type { QrScannerProps } from './qr-scanner.types';

/** Web fallback: no camera scanner in the browser build, so the screen is
 * just the manual-entry path the previous placeholder offered. */
export function QrScanner(props: QrScannerProps) {
  return (
    <ScanFallback
      icon={{ ios: 'qrcode.viewfinder', android: 'qr_code_scanner', web: 'qr_code_scanner' }}
      message="Camera scanning isn’t available in this build yet. Enter the daemon address and token by hand instead."
      onManual={props.onManual}
    />
  );
}
