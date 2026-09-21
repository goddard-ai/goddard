import * as Haptics from "expo-haptics";
import { router, Stack, useLocalSearchParams } from "expo-router";
import { useEffect, useRef, useState } from "react";
import { ActivityIndicator, Pressable, StyleSheet, Text, View } from "react-native";

import { AppSymbol } from "@/components/app-symbol";
import { navigateBack } from "@/components/screen-header";
import { NativeTint, Radius } from "@/constants/theme";
import { useTheme } from "@/hooks/use-theme";
import { useDaemon } from "@/lib/daemon-context";
import { daemonConnectLink } from "@/lib/daemon-profile";

/** Handles `goddard://connect?address=…&token=…` — the link inside the
 * desktop daemon settings QR — whether it arrives from the system camera,
 * another app's scanner, or the in-app scanner route. Saves the profile,
 * activates it, and lands on the task list. */
export default function ConnectScreen() {
  const theme = useTheme();
  const daemon = useDaemon();
  const params = useLocalSearchParams<{
    address?: string | string[];
    token?: string | string[];
    name?: string | string[];
  }>();
  const [error, setError] = useState<string | null>(null);
  const attempted = useRef(false);

  useEffect(() => {
    if (attempted.current) return;
    attempted.current = true;
    const first = (value?: string | string[]) =>
      Array.isArray(value) ? value[0] : value;
    void (async () => {
      try {
        const link = daemonConnectLink({
          address: first(params.address),
          token: first(params.token),
          name: first(params.name),
        });
        const result = await daemon.saveProfile(link);
        await Haptics.notificationAsync(
          result.connected
            ? Haptics.NotificationFeedbackType.Success
            : Haptics.NotificationFeedbackType.Warning,
        );
        router.replace("/");
      } catch (cause) {
        setError(cause instanceof Error ? cause.message : String(cause));
        await Haptics.notificationAsync(Haptics.NotificationFeedbackType.Error);
      }
    })();
  }, []);

  return (
    <View style={[styles.screen, { backgroundColor: theme.background }]}>
      <Stack.Screen options={{ title: "Connect" }} />
      {error ? (
        <>
          <View style={styles.messageRow}>
            <AppSymbol
              name={{
                ios: "exclamationmark.circle.fill",
                android: "error",
                web: "error",
              }}
              size={16}
              tintColor={theme.danger}
            />
            <Text style={[styles.messageText, { color: theme.danger }]}>
              {error}
            </Text>
          </View>
          <View style={styles.actions}>
            <Pressable
              accessibilityRole="button"
              onPress={() => router.push("/daemon-editor")}
              style={({ pressed }) => [
                styles.button,
                { backgroundColor: NativeTint, opacity: pressed ? 0.6 : 1 },
              ]}
            >
              <Text style={styles.buttonText}>Enter manually</Text>
            </Pressable>
            <Pressable
              accessibilityRole="button"
              onPress={navigateBack}
              style={({ pressed }) => [
                styles.button,
                { backgroundColor: theme.overlay, opacity: pressed ? 0.6 : 1 },
              ]}
            >
              <Text style={[styles.buttonText, { color: theme.text }]}>
                Cancel
              </Text>
            </Pressable>
          </View>
        </>
      ) : (
        <>
          <ActivityIndicator color={NativeTint} />
          <Text style={[styles.status, { color: theme.textSecondary }]}>
            Connecting to daemon…
          </Text>
        </>
      )}
    </View>
  );
}

const styles = StyleSheet.create({
  screen: {
    alignItems: "center",
    flex: 1,
    gap: 14,
    justifyContent: "center",
    padding: 32,
  },
  status: { fontSize: 16 },
  messageRow: {
    alignItems: "flex-start",
    flexDirection: "row",
    gap: 8,
    maxWidth: 340,
  },
  messageText: { flex: 1, fontSize: 15, lineHeight: 20 },
  actions: { flexDirection: "row", gap: 10, marginTop: 8 },
  button: {
    alignItems: "center",
    borderRadius: Radius.medium,
    justifyContent: "center",
    minHeight: 44,
    paddingHorizontal: 18,
  },
  buttonText: { color: "#ffffff", fontSize: 16, fontWeight: "600" },
});
