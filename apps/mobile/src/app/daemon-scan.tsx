import { CameraView, useCameraPermissions } from "expo-camera";
import * as Haptics from "expo-haptics";
import { Stack } from "expo-router";
import type { SymbolViewProps } from "expo-symbols";
import { useEffect, useRef, useState } from "react";
import {
  ActivityIndicator,
  Linking,
  Platform,
  Pressable,
  StyleSheet,
  Text,
  View,
} from "react-native";

import { AppSymbol } from "@/components/app-symbol";
import { navigateBack } from "@/components/screen-header";
import { NativeTint, Radius } from "@/constants/theme";
import { useTheme } from "@/hooks/use-theme";
import { useDaemon } from "@/lib/daemon-context";
import {
  parseDaemonConnectLink,
  type DaemonConnectLink,
} from "@/lib/daemon-profile";

/** Camera scanner for the `goddard://connect` QR the desktop shows in
 * Settings → Daemon. A good scan saves the profile and connects; anything
 * else reports inline and keeps scanning. */
export default function DaemonScanScreen() {
  const theme = useTheme();
  const daemon = useDaemon();
  const [permission, requestPermission] = useCameraPermissions();
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const handled = useRef(false);

  useEffect(() => {
    if (Platform.OS === "web") return;
    if (permission && !permission.granted && permission.canAskAgain) {
      void requestPermission();
    }
  }, [permission, requestPermission]);

  async function onScanned(data: string) {
    if (handled.current || saving) return;
    let link: DaemonConnectLink;
    try {
      link = parseDaemonConnectLink(data);
    } catch (cause) {
      // A stray QR that isn't ours: say so, but keep the scanner live.
      setError(cause instanceof Error ? cause.message : String(cause));
      return;
    }
    handled.current = true;
    setSaving(true);
    setError(null);
    try {
      const result = await daemon.saveProfile(link);
      await Haptics.notificationAsync(
        result.connected
          ? Haptics.NotificationFeedbackType.Success
          : Haptics.NotificationFeedbackType.Warning,
      );
      navigateBack();
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : String(cause));
      await Haptics.notificationAsync(Haptics.NotificationFeedbackType.Error);
      handled.current = false;
    } finally {
      setSaving(false);
    }
  }

  const granted = permission?.granted ?? false;
  const blocked = permission ? !permission.canAskAgain && !granted : false;

  return (
    <View style={[styles.screen, { backgroundColor: theme.background }]}>
      <Stack.Screen
        options={{ presentation: "pageSheet", title: "Scan QR Code" }}
      />
      {Platform.OS === "web" ? (
        <Message
          icon={{ ios: "info.circle", android: "info", web: "info" }}
          text="QR scanning needs the Goddard mobile app on iOS or Android."
          color={theme.textSecondary}
        />
      ) : !permission || (!granted && !blocked) ? (
        <View style={styles.center}>
          <ActivityIndicator color={NativeTint} />
        </View>
      ) : blocked ? (
        <View style={styles.center}>
          <Message
            icon={{
              ios: "camera.fill",
              android: "photo_camera",
              web: "photo_camera",
            }}
            text="Goddard needs camera access to scan the code shown in Settings → Daemon."
            color={theme.textSecondary}
          />
          <Pressable
            accessibilityRole="button"
            onPress={() => void Linking.openSettings()}
            style={({ pressed }) => [
              styles.button,
              { backgroundColor: NativeTint, opacity: pressed ? 0.6 : 1 },
            ]}
          >
            <Text style={styles.buttonText}>Open Settings</Text>
          </Pressable>
        </View>
      ) : (
        <View style={styles.cameraWrap}>
          <CameraView
            style={styles.camera}
            facing="back"
            barcodeScannerSettings={{ barcodeTypes: ["qr"] }}
            onBarcodeScanned={
              saving ? undefined : (event) => void onScanned(event.data)
            }
          />
          <View pointerEvents="none" style={styles.viewfinder} />
          {saving && (
            <View style={styles.savingOverlay}>
              <ActivityIndicator color="#ffffff" />
              <Text style={styles.savingText}>Connecting…</Text>
            </View>
          )}
        </View>
      )}
      {error && (
        <View style={styles.errorRow}>
          <AppSymbol
            name={{
              ios: "exclamationmark.circle.fill",
              android: "error",
              web: "error",
            }}
            size={15}
            tintColor={theme.danger}
          />
          <Text style={[styles.errorText, { color: theme.danger }]}>
            {error}
          </Text>
        </View>
      )}
      <Text style={[styles.hint, { color: theme.textTertiary }]}>
        The code is in Goddard Desktop → Settings → Daemon → Show QR code.
      </Text>
    </View>
  );
}

function Message({
  color,
  icon,
  text,
}: {
  color: string;
  icon: SymbolViewProps["name"];
  text: string;
}) {
  return (
    <View style={styles.messageRow}>
      <AppSymbol name={icon} size={16} tintColor={color} />
      <Text style={[styles.messageText, { color }]}>{text}</Text>
    </View>
  );
}

const styles = StyleSheet.create({
  screen: { flex: 1, padding: 20 },
  center: { alignItems: "center", flex: 1, gap: 16, justifyContent: "center" },
  cameraWrap: {
    alignItems: "center",
    borderRadius: Radius.large,
    flex: 1,
    justifyContent: "center",
    overflow: "hidden",
  },
  camera: {
    bottom: 0,
    left: 0,
    position: "absolute",
    right: 0,
    top: 0,
  },
  viewfinder: {
    borderColor: "#ffffff",
    borderRadius: 20,
    borderWidth: 2.5,
    height: 240,
    opacity: 0.85,
    width: 240,
  },
  savingOverlay: {
    alignItems: "center",
    bottom: 0,
    left: 0,
    position: "absolute",
    right: 0,
    top: 0,
    backgroundColor: "rgba(0,0,0,0.55)",
    gap: 10,
    justifyContent: "center",
  },
  savingText: { color: "#ffffff", fontSize: 15, fontWeight: "600" },
  messageRow: {
    alignItems: "flex-start",
    flexDirection: "row",
    gap: 8,
    maxWidth: 340,
  },
  messageText: { flex: 1, fontSize: 14, lineHeight: 20 },
  button: {
    alignItems: "center",
    borderRadius: Radius.medium,
    justifyContent: "center",
    minHeight: 44,
    paddingHorizontal: 18,
  },
  buttonText: { color: "#ffffff", fontSize: 15, fontWeight: "600" },
  errorRow: {
    alignItems: "flex-start",
    flexDirection: "row",
    gap: 8,
    marginTop: 14,
  },
  errorText: { flex: 1, fontSize: 13, lineHeight: 18 },
  hint: { fontSize: 13, lineHeight: 18, marginTop: 12, textAlign: "center" },
});
