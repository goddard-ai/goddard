import { router, type NativeStackHeaderItem } from "expo-router";
import { useHeaderHeight } from "expo-router/build/react-navigation/elements";
import type { ReactNode } from "react";
import {
  Pressable,
  StyleSheet,
  Text,
  View,
  useWindowDimensions,
} from "react-native";

import { AppSymbol } from "./app-symbol";
import { GlassSurface } from "./glass-surface";
import { Radius } from "@/constants/theme";
import { useTheme } from "@/hooks/use-theme";

/** Pop when there is history; otherwise land on the task list. A screen
 * opened cold (deep link, state restore) is the stack's only entry, and a
 * bare router.back() there throws GO_BACK unhandled. */
export function navigateBack() {
  if (router.canGoBack()) router.back();
  else router.replace("/");
}

/**
 * Bottom edge of the transparent native navigation bar, measured from the top
 * of the screen. Content that runs under the bar insets by this. Rounded so
 * the native height report, a fraction off the JS default, does not re-layout
 * the transcript once it lands.
 */
export function useScreenHeaderInset() {
  return Math.round(useHeaderHeight());
}

/**
 * Solid surface behind the transparent navigation bar: background fill plus a
 * hairline, never a blur or gradient. Always mounted — the transcript frame
 * is inset below it, so it can only ever cover the empty header strip. It
 * lives in the screen content, so it travels with the page during a
 * swipe-back while the bar's buttons and title stay put in the native
 * navigation bar above it.
 */
export function ScreenHeaderBackdrop() {
  const theme = useTheme();
  const height = useScreenHeaderInset();
  return (
    <View
      pointerEvents="none"
      style={[
        styles.backdrop,
        {
          backgroundColor: theme.background,
          borderBottomColor: theme.borderStrong,
          height,
        },
      ]}
    />
  );
}

/** Space occupied by the leading button, trailing actions, native margins,
 * and the gaps around a task title placed with the leading bar items. */
const TitleChromeWidth = 208;
const TitleMinWidth = 56;
const TitleMaxWidth = 360;

/** Two-line task label for the native navigation bar. iOS mounts it beside
 * the leading button; other platforms use it as their left-aligned title. */
export function HeaderTitle({
  title,
  subtitle,
}: {
  title: string;
  subtitle?: string | null;
}) {
  const theme = useTheme();
  const { width } = useWindowDimensions();
  const maxWidth = Math.min(
    TitleMaxWidth,
    Math.max(TitleMinWidth, width - TitleChromeWidth),
  );
  return (
    <View style={[styles.titles, { maxWidth }]}>
      <Text numberOfLines={1} style={[styles.title, { color: theme.text }]}>
        {title}
      </Text>
      {subtitle ? (
        <Text
          numberOfLines={1}
          style={[styles.subtitle, { color: theme.textTertiary }]}
        >
          {subtitle}
        </Text>
      ) : null}
    </View>
  );
}

export type HeaderActionSpec = {
  icon: Parameters<typeof AppSymbol>[0]["name"];
  label: string;
  onPress: () => void;
  /** New-content dot on the glyph — unseen replies in another task. */
  unseen?: boolean;
};

/** Native bar button items for iOS: SF Symbol glyphs in the system's shared
 * Liquid Glass capsule, grouped and transitioned by UIKit. */
export function nativeHeaderButtons(
  actions: HeaderActionSpec[],
): NativeStackHeaderItem[] {
  return actions.map((action) => {
    if (action.unseen) {
      // UIBarButtonItemBadge renders text only — never the bare indicator —
      // so a flagged action mounts as a custom view instead. The shared
      // capsule still wraps it (hidesSharedBackground stays unset).
      return {
        type: "custom",
        element: <HeaderBarAction {...action} />,
      };
    }
    const symbol =
      typeof action.icon === "string" ? action.icon : action.icon.ios;
    return {
      type: "button",
      label: action.label,
      accessibilityLabel: action.label,
      ...(symbol ? { icon: { type: "sfSymbol", name: symbol } } : {}),
      onPress: action.onPress,
    };
  });
}

/** Pill grouping trailing header actions for platforms without native bar
 * button items, like the reference's [compose | …]. */
export function HeaderActionGroup({ children }: { children: ReactNode }) {
  return (
    <GlassSurface interactive style={styles.actionGroup}>
      {children}
    </GlassSurface>
  );
}

export function HeaderAction({ icon, label, unseen, onPress }: HeaderActionSpec) {
  const theme = useTheme();
  return (
    <Pressable
      accessibilityLabel={label}
      accessibilityRole="button"
      hitSlop={4}
      onPress={onPress}
      style={({ pressed }) => [styles.action, { opacity: pressed ? 0.5 : 1 }]}
    >
      <AppSymbol name={icon} size={17} tintColor={theme.text} />
      {unseen ? (
        <View style={[styles.actionDot, { backgroundColor: theme.info }]} />
      ) : null}
    </Pressable>
  );
}

/** Glyph-sized pressable for `custom` header items: a native `button` item
 * can't carry the unseen dot, and the shared capsule UIKit draws around the
 * view already supplies the chrome — so the frame hugs the glyph instead of
 * HeaderAction's 44pt touch box. */
function HeaderBarAction({ icon, label, unseen, onPress }: HeaderActionSpec) {
  const theme = useTheme();
  return (
    <Pressable
      accessibilityLabel={label}
      accessibilityRole="button"
      hitSlop={8}
      onPress={onPress}
      style={({ pressed }) => [styles.barAction, { opacity: pressed ? 0.5 : 1 }]}
    >
      <AppSymbol name={icon} scale="large" size={22} tintColor={theme.text} />
      {unseen ? (
        <View style={[styles.barActionDot, { backgroundColor: theme.info }]} />
      ) : null}
    </Pressable>
  );
}

/** Non-pressable trigger content for a platform-native menu. MenuView owns
 * the gesture recognizer; keeping this as a View avoids nested pressables on
 * Android while preserving one labelled accessibility target. */
export function HeaderMenuTrigger({
  icon,
  label,
}: Pick<HeaderActionSpec, "icon" | "label">) {
  const theme = useTheme();
  return (
    <View
      accessible
      accessibilityLabel={label}
      accessibilityRole="button"
      style={styles.action}
    >
      <AppSymbol name={icon} size={17} tintColor={theme.text} />
    </View>
  );
}

const styles = StyleSheet.create({
  backdrop: {
    borderBottomWidth: StyleSheet.hairlineWidth,
    left: 0,
    position: "absolute",
    right: 0,
    top: 0,
    zIndex: 20,
  },
  titles: {
    alignItems: "flex-start",
    justifyContent: "center",
    minWidth: 0,
    overflow: "hidden",
  },
  title: {
    fontSize: 18,
    fontWeight: "700",
    letterSpacing: -0.3,
    textAlign: "left",
  },
  subtitle: {
    fontSize: 13.5,
    marginTop: 1,
    textAlign: "left",
  },
  actionGroup: {
    alignItems: "center",
    borderRadius: Radius.pill,
    flexDirection: "row",
    height: 44,
    paddingHorizontal: 4,
  },
  action: {
    alignItems: "center",
    height: 44,
    justifyContent: "center",
    width: 42,
  },
  actionDot: {
    borderRadius: 4,
    height: 8,
    position: "absolute",
    right: 8,
    top: 10,
    width: 8,
  },
  barAction: {
    alignItems: "center",
    height: 30,
    justifyContent: "center",
    width: 30,
  },
  barActionDot: {
    borderRadius: 4,
    height: 8,
    position: "absolute",
    right: 0,
    top: 0,
    width: 8,
  },
});
