/** @jsxImportSource @nativescript-community/octane */
import { registerElement } from '@nativescript-community/octane';
import type { Attributes } from '@nativescript-community/octane/intrinsics';
import { BarcodeFormats, DetectionType, MLKitView, type DetectionEvent } from '@nativescript/mlkit-core';
import type { BarcodeResult } from '@nativescript/mlkit-barcode-scanning';
import type { EventData } from '@nativescript/core';
import { useEffect, useRef, useState } from 'octane';
import { Grid, Pressable, Text, useSafeAreaInsets, View } from '@octane-xplat/ui';

import { ScanFallback } from './scan-fallback';
import type { QrScannerProps } from './qr-scanner.types';
import { NativeTint } from '../lib/theme-platform';
import { Radius } from '../lib/theme-colors';

registerElement('mlkitview', MLKitView);

declare module '@nativescript-community/octane/intrinsics' {
  interface NativeScriptElements {
    mlkitview: Attributes<typeof MLKitView>;
  }
}

/** Live QR scanner for `goddard://connect` codes — the MLKit camera view
 * fills the screen behind a viewfinder and the manual-entry hatch. A denied
 * permission drops to the same manual path the web leaf always shows. */
export function QrScanner(props: QrScannerProps) {
  const insets = useSafeAreaInsets();
  const scanner = useRef<MLKitView | null>(null);
  const seen = useRef(false);
  const [denied, setDenied] = useState(false);

  useEffect(() => {
    const view = scanner.current;
    if (!view || view.hasCameraPermission()) {return;}
    let mounted = true;
    void view.requestCameraPermission().catch(() => {
      if (mounted) {setDenied(true);}
    });

    return () => {
      mounted = false;
    };
  }, []);

  const onDetection = (event: EventData) => {
    const detection = event as DetectionEvent;
    if (seen.current || detection.type !== DetectionType.Barcode) {return;}
    const codes = Array.isArray(detection.data) ? detection.data : [detection.data];
    const raw = (codes[0] as BarcodeResult | undefined)?.rawValue;
    if (!raw) {return;}
    seen.current = true;
    props.onCode(raw);
  };

  if (denied) {
    return (
      <ScanFallback
        icon={{ ios: 'camera.fill', android: 'photo_camera', web: 'photo_camera' }}
        message="Camera access is needed to scan the pairing code. You can enter the daemon address and token by hand instead."
        onManual={props.onManual}
      />
    );
  }

  return (
    <Grid rows="*" columns="*" style={{ flex: '1 1', backgroundColor: '#000000' }}>
      <View row={0} col={0} style={{ flex: '1 1' }}>
        <mlkitview
          ref={scanner}
          detectionType={DetectionType.Barcode}
          barcodeFormats={[BarcodeFormats.QR_CODE]}
          style={{ flex: '1 1' }}
          onDetection={onDetection}
        />
      </View>
      <View
        row={0}
        col={0}
        style={{ alignItems: 'center', justifyContent: 'center' }}
      >
        <View
          style={{
            borderColor: 'rgba(255, 255, 255, 0.9)',
            borderRadius: Radius.large,
            borderWidth: 3,
            height: 240,
            width: 240,
          }}
        />
      </View>
      <View
        row={0}
        col={0}
        style={{
          alignItems: 'center',
          gap: 14,
          justifyContent: 'flex-end',
          padding: 24,
          paddingBottom: insets.bottom + 24,
        }}
      >
        <Text style={{ fontSize: 15, textAlign: 'center', color: '#ffffff' }}>
          Scan the pairing code in Settings → Daemon on your host
        </Text>
        <Pressable
          accessibilityRole="button"
          onPress={props.onManual}
          className="vx-pressable"
          style={{
            alignItems: 'center',
            borderRadius: Radius.medium,
            justifyContent: 'center',
            minHeight: 44,
            paddingHorizontal: 18,
            backgroundColor: String(NativeTint),
          }}
        >
          <Text style={{ color: '#ffffff', fontSize: 16, fontWeight: '600' }}>Enter manually</Text>
        </Pressable>
      </View>
    </Grid>
  );
}
