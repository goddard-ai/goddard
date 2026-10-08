/** @jsxImportSource @nativescript-community/octane */
import type { MessageAttachment } from '@waku/client';
import { memo, useCallback, useState } from 'octane';
import {
  Absolute,
  Spinner,
  Image,
  Pressable,
  BottomSheet as XSheet,
  Text,
  View,
} from '@octane-xplat/ui';

import { AppSymbol } from '@/components/app-symbol';
import { dismissKeyboard } from '@/lib/keyboard';
import { useTheme } from '@/lib/use-theme';
import { attachmentImageQuery } from '@/lib/attachments';
import { useDaemon, useDaemonQuery } from '@/lib/daemon-hooks';

/** Fixed-size previews keep the composer and transcript steady while loading.
 * The remove target is a sibling of the preview, so tapping an image is safe. */
export const AttachmentTile = memo(function AttachmentTile({
  attachment,
  compact = false,
  onRemove,
  removeDisabled = false,
}: {
  attachment: MessageAttachment;
  compact?: boolean;
  onRemove?: () => void;
  removeDisabled?: boolean;
}) {
  const theme = useTheme();
  const { activeProfile, client, phase } = useDaemon();
  const options = attachmentImageQuery(client, activeProfile, attachment, phase === 'connected');
  const image = useDaemonQuery(options.queryKey, options.queryFn, {
    enabled: options.enabled,
    staleTime: options.staleTime,
  });

  const [failedSource, setFailedSource] = useState<string | null>(null);
  const [preview, setPreview] = useState<{ identity: string; source: string } | null>(null);
  const identity = JSON.stringify(options.queryKey);
  const source = attachment.is_image && !attachment.is_dir && image.data && image.data !== failedSource
    ? image.data : null;

  const canRetry = Boolean(image.error) && options.enabled;
  const interactive = Boolean(source || canRetry);
  const onImageError = useCallback(() => setFailedSource(image.data ?? null), [image.data]);

  if (attachment.session_id) {
    return (
      <View style={{
        height: 24,
        maxWidth: 240,
        flexDirection: 'row',
        alignItems: 'center',
        gap: 5,
        borderRadius: 8,
        borderWidth: 1,
        paddingLeft: 6,
        paddingRight: 4,
        backgroundColor: theme.inset,
        borderColor: theme.border,
      }}>
        <AppSymbol
          name={{ ios: 'text.bubble', android: 'chat_bubble', web: 'chat' }}
          size={11}
          tintColor={theme.textTertiary}
        />
        <Text
          numberOfLines={1}
          style={{ fontSize: 13.5, flexShrink: 1, color: theme.textSecondary }}>
          {attachment.name}
        </Text>
        {onRemove && (
          <Pressable
            accessibilityLabel={`Remove ${attachment.name}`}
            accessibilityRole="button"
            accessibilityState={{ disabled: removeDisabled }}
            disabled={removeDisabled}
            onPress={onRemove}
            style={{
              width: 28,
              height: 28,
              alignItems: 'center',
              justifyContent: 'center',
              opacity: removeDisabled ? 0.4 : 1,
            }}>
            <AppSymbol name={{ ios: 'xmark', android: 'close', web: 'close' }} size={9} tintColor={theme.textSecondary} />
          </Pressable>
        )}
      </View>
    );
  }

  return (
    <View style={{ width: compact ? 80 : 96, height: 80, flexShrink: 0 }}>
      <Pressable
        accessibilityLabel={source ? `Preview ${attachment.name}`
          : canRetry ? `Retry preview for ${attachment.name}` : attachment.name}
        accessibilityRole={interactive ? 'button' : 'text'}
        disabled={!interactive}
        onPress={() => {
          if (source) {
            dismissKeyboard();
            setPreview({ identity, source });
          } else if (canRetry) {void image.refetch();}
        }}
        style={{
          flex: '1 1',
          borderRadius: 9,
          borderWidth: 1,
          overflow: 'hidden',
          backgroundColor: theme.inset,
          borderColor: theme.border,
        }}>
        {source ? (
          <AttachmentImage source={source} onError={onImageError} />
        ) : (
          <View style={{ flex: '1 1', alignItems: 'center', justifyContent: 'center', gap: 7, paddingHorizontal: 7 }}>
            {image.isFetching ? <Spinner busy /> : (
              <AppSymbol
                name={attachment.is_dir
                  ? { ios: 'folder', android: 'folder', web: 'folder' }
                  : attachment.is_image
                    ? { ios: 'photo', android: 'image', web: 'image' }
                    : { ios: 'doc', android: 'description', web: 'description' }}
                size={18}
                tintColor={theme.textTertiary}
              />
            )}
            <Text numberOfLines={1} style={{ alignSelf: 'stretch', fontSize: 12.5, textAlign: 'center', color: theme.textSecondary }}>
              {attachment.name}
            </Text>
            {(canRetry || failedSource === image.data) && (
              <Text style={{ fontSize: 11, marginTop: -4, color: theme.textTertiary }}>
                {canRetry ? 'Tap to retry' : 'No preview'}
              </Text>
            )}
          </View>
        )}
      </Pressable>
      {onRemove && (
        <Absolute style={{ top: 0, right: 0 }}>
        <Pressable
          accessibilityLabel={`Remove ${attachment.name}`}
          accessibilityRole="button"
          accessibilityState={{ disabled: removeDisabled }}
          disabled={removeDisabled}
          onPress={onRemove}
          style={{
            width: 44,
            height: 44,
            alignItems: 'flex-end',
            padding: 3,
            opacity: removeDisabled ? 0.4 : 1,
          }}>
          <View style={{
            width: 26,
            height: 26,
            borderRadius: 13,
            borderWidth: 1,
            alignItems: 'center',
            justifyContent: 'center',
            backgroundColor: theme.surface,
            borderColor: theme.borderStrong,
          }}>
            <AppSymbol name={{ ios: 'xmark', android: 'close', web: 'close' }} size={11} tintColor={theme.text} />
          </View>
        </Pressable>
        </Absolute>
      )}
      {preview?.identity === identity && preview.source === source && (
        <AttachmentImagePreview
          name={attachment.name}
          source={preview.source}
          onDismiss={() => setPreview(null)}
        />
      )}
    </View>
  );
});

function AttachmentImage({
  source,
  contain = false,
  onError: _onError,
}: {
  source: string;
  contain?: boolean;
  onError: () => void;
}) {
  // TODO(port): SVG data URIs rendered through react-native-svg before;
  // the shared Image may not rasterize them on native — flag if previews
  // show blank for .svg attachments.
  return (
    <Image
      src={source}
      style={{ width: '100%', height: '100%' }}
      ios={{ stretch: contain ? 'aspectFit' : 'aspectFill' }}
      web={{ style: { objectFit: contain ? 'contain' : 'cover' } }}
    />
  );
}

function AttachmentImagePreview({
  name,
  source,
  onDismiss,
  onError,
}: {
  name: string;
  source: string;
  onDismiss: () => void;
  onError?: () => void;
}) {
  const theme = useTheme();
  return (
    <XSheet label="Attachment preview" isOpen onOpenChange={(isOpen) => {if (!isOpen) {onDismiss();}}} snapPoints={[1]}>
      <View style={{ flex: '1 1', backgroundColor: theme.background }}>
        <View style={{
          flexDirection: 'row',
          alignItems: 'center',
          gap: 12,
          paddingHorizontal: 16,
          paddingVertical: 8,
        }}>
          <Text numberOfLines={1} style={{ flex: '1 1', fontSize: 16, fontWeight: '600', color: theme.text }}>{name}</Text>
          <Pressable
            accessibilityLabel="Close image preview"
            accessibilityRole="button"
            onPress={onDismiss}
            style={{
              width: 36,
              height: 36,
              borderRadius: 18,
              alignItems: 'center',
              justifyContent: 'center',
              backgroundColor: theme.raised,
            }}>
            <AppSymbol name={{ ios: 'xmark', android: 'close', web: 'close' }} size={17} tintColor={theme.text} />
          </Pressable>
        </View>
        <View accessibilityLabel={name} accessibilityRole="image" style={{ flex: '1 1' }}>
          <AttachmentImage contain source={source} onError={onError ?? (() => {})} />
        </View>
      </View>
    </XSheet>
  );
}
