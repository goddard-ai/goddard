export interface QrScannerProps {
  /** Raw payload of the first decoded code; the caller validates it. */
  onCode: (value: string) => void;
  /** The user picked manual entry instead of scanning. */
  onManual: () => void;
}
