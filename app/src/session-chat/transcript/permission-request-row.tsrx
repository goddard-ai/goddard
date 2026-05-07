import { AlertCircle, CheckCircle2, ShieldAlert, XCircle } from "lucide-react"
import { useState } from "preact/hooks"

import type {
  SessionTranscriptPermissionOption,
  SessionTranscriptPermissionRequest,
} from "~/sessions/models.ts"
import {
  permissionActionRowClass,
  permissionCardClass,
  permissionDetailBlockClass,
  permissionDetailGridClass,
  permissionDetailLabelClass,
  permissionDetailMonoClass,
  permissionDetailTextClass,
  permissionErrorClass,
  permissionHeaderClass,
  permissionIconClass,
  permissionMetaClass,
  permissionOptionButtonClass,
  permissionOptionKindClass,
  permissionOptionNameClass,
  permissionResolutionClass,
  permissionStatusBadgeClass,
  permissionTitleClass,
  permissionTitleGroupClass,
  rowClass,
  rowInnerClass,
} from "./transcript.style.ts"

function getPermissionStatusIcon(status: SessionTranscriptPermissionRequest["status"]) {
  switch (status) {
    case "allowed":
    case "resolved":
      return CheckCircle2
    case "denied":
    case "cancelled":
      return XCircle
    case "failed":
      return AlertCircle
    default:
      return ShieldAlert
  }
}

function formatPermissionStatusLabel(status: SessionTranscriptPermissionRequest["status"]) {
  switch (status) {
    case "allowed":
      return "Allowed"
    case "denied":
      return "Denied"
    case "resolved":
      return "Resolved"
    case "cancelled":
      return "Cancelled"
    case "failed":
      return "Failed"
    default:
      return "Permission needed"
  }
}

function formatPermissionToolKind(toolKind: SessionTranscriptPermissionRequest["toolKind"]) {
  return toolKind === "switch_mode"
    ? "Switch mode"
    : `${toolKind.slice(0, 1).toUpperCase()}${toolKind.slice(1)}`
}

function formatPermissionOptionKind(kind: SessionTranscriptPermissionOption["kind"]) {
  switch (kind) {
    case "allow_always":
      return "Allow always"
    case "allow_once":
      return "Allow once"
    case "reject_always":
      return "Reject always"
    case "reject_once":
      return "Reject once"
  }
}

function resolvePermissionOptionIntent(kind: SessionTranscriptPermissionOption["kind"]) {
  return kind.startsWith("reject_") ? "reject" : "allow"
}

function findSelectedPermissionOption(message: SessionTranscriptPermissionRequest) {
  if (!message.selectedOptionId) {
    return null
  }

  return message.options.find((option) => option.optionId === message.selectedOptionId) ?? null
}

function formatPermissionResolution(message: SessionTranscriptPermissionRequest) {
  const selectedOption = findSelectedPermissionOption(message)
  const selectedLabel = selectedOption?.name ?? message.selectedOptionId ?? "selected option"

  switch (message.status) {
    case "allowed":
      return `Allowed: ${selectedLabel}`
    case "denied":
      return `Denied: ${selectedLabel}`
    case "resolved":
      return `Resolved: ${selectedLabel}`
    case "cancelled":
      return "Cancelled before a permission option was selected."
    case "failed":
      return message.error ?? "The permission response failed."
    default:
      return message.options.length > 0
        ? "Waiting for a permission option."
        : "Waiting for a permission response."
  }
}

function formatPermissionActionError(error: unknown) {
  if (error instanceof Error && error.message.trim().length > 0) {
    return error.message
  }

  return "The daemon rejected the permission response."
}

component PermissionTargets(props: {
  message: SessionTranscriptPermissionRequest;
}) {
  if (props.message.locations.length === 0 && !props.message.context) {
    return
  }

  <div class={permissionDetailGridClass}>
    if (props.message.locations.length > 0) {
      <div class={permissionDetailBlockClass}>
        <div class={permissionDetailLabelClass}>{"Target"}</div>
        for (const location of props.message.locations; index index) {
          <div
            class={permissionDetailMonoClass}
            key={`${location.path}:${location.line ?? "none"}:${index}`}
          >
            {location.path}
            {location.line != null ? `:${location.line}` : ""}
          </div>
        }
      </div>
    }
    if (props.message.context) {
      <div class={permissionDetailBlockClass}>
        <div class={permissionDetailLabelClass}>{"Context"}</div>
        <div class={permissionDetailTextClass}>
          {props.message.context}
        </div>
      </div>
    }
  </div>
}

/** Renders one ACP permission request and sends the selected option through the session transport. */
export component PermissionRequestTranscriptRow(props: {
  message: SessionTranscriptPermissionRequest;
  onSelectOption?: (input: {
    optionId: string;
    requestId: SessionTranscriptPermissionRequest["requestId"];
  }) => Promise<void> | void;
}) {
  const [pendingOptionId, setPendingOptionId] = useState<string | null>(null)
  const [error, setError] = useState<string | null>(null)
  const StatusIcon = getPermissionStatusIcon(props.message.status)
  const hasActions =
    props.message.status === "pending" && props.message.options.length > 0 && props.onSelectOption

  <article class={rowClass}>
    <div class={rowInnerClass} style={{ justifyContent: "flex-start" }}>
      <div class={permissionCardClass}>
        <header class={permissionHeaderClass}>
          <span class={permissionIconClass} aria-hidden="true">
            <ShieldAlert size={16} strokeWidth={2.2} />
          </span>
          <div class={permissionTitleGroupClass}>
            <h3 class={permissionTitleClass}>{props.message.title}</h3>
            <div class={permissionMetaClass}>
              {formatPermissionToolKind(props.message.toolKind)}
            </div>
          </div>
          <span class={permissionStatusBadgeClass} data-status={props.message.status}>
            <StatusIcon size={12} strokeWidth={2.2} />
            {formatPermissionStatusLabel(props.message.status)}
          </span>
        </header>

        <PermissionTargets message={props.message} />

        if (hasActions) {
          <div class={permissionActionRowClass}>
            for (const option of props.message.options) {
              <button
                aria-label={`${formatPermissionOptionKind(option.kind)}: ${option.name}`}
                class={permissionOptionButtonClass}
                data-intent={resolvePermissionOptionIntent(option.kind)}
                disabled={pendingOptionId !== null}
                key={option.optionId}
                type="button"
                onClick={() => {
                  void (async () => {
                    setPendingOptionId(option.optionId)
                    setError(null)

                    try {
                      await props.onSelectOption?.(
                        {
                          requestId: props.message.requestId,
                          optionId: option.optionId,
                        },
                      )
                    } catch (error) {
                      console.error("Failed to respond to permission request.", error)
                      setError(formatPermissionActionError(error))
                    } finally {
                      setPendingOptionId(null)
                    }
                  })()
                }}
              >
                <span class={permissionOptionNameClass}>{option.name}</span>
                <span class={permissionOptionKindClass}>
                  {pendingOptionId === option.optionId
                    ? "Sending"
                    : formatPermissionOptionKind(option.kind)}
                </span>
              </button>
            }
          </div>
        } else {
          <div class={permissionResolutionClass}>
            {formatPermissionResolution(props.message)}
          </div>
        }

        if (error && props.message.status === "pending") {
          <div class={permissionErrorClass} role="alert">
            {error}
          </div>
        }
      </div>
    </div>
  </article>
}
