import { t } from "@lingui/core/macro"
import {
  Bot,
  Brain,
  CheckCircle2,
  ChevronDown,
  ChevronLeft,
  ChevronRight,
  ChevronUp,
  Command,
  FileDiff,
  Folder,
  FolderOpen,
  GitBranch,
  History,
  Inbox,
  Keyboard,
  Lightbulb,
  ListChecks,
  ListTodo,
  Map,
  MapPin,
  MessageSquarePlus,
  PanelTopClose,
  Search,
  SendHorizontal,
  Settings,
  Terminal,
} from "lucide-react"

import { defineAppCommands } from "./app-command.ts"

/** App command definitions surfaced to shortcut bindings and the command menu. */
export const AppCommand = defineAppCommands({
  workbench: {
    group: "workbench",
    commands: {
      closeActiveTab: {
        get label() {
          return t`Close Active Tab`
        },
        icon: PanelTopClose,
        when: "workbench.hasClosableActiveTab",
      },
      navigateBack: {
        get label() {
          return t`Navigate Back`
        },
        icon: ChevronLeft,
        when: "workbench.canNavigateBack",
      },
      navigateForward: {
        get label() {
          return t`Navigate Forward`
        },
        icon: ChevronRight,
        when: "workbench.canNavigateForward",
      },
      openRecentTabs: {
        get label() {
          return t`Open Recent Tabs`
        },
        icon: History,
      },
    },
  },
  navigation: {
    group: "navigation",
    commands: {
      openProposeTaskDialog: {
        get label() {
          return t`Open Propose Task Dialog`
        },
        icon: Lightbulb,
      },
      openNewSessionDialog: {
        get label() {
          return t`Open New Session Dialog`
        },
        icon: MessageSquarePlus,
      },
      openTerminal: {
        label: "Open Terminal",
        icon: Terminal,
        keywords: ["shell", "pty"],
      },
      openSwitchProject: {
        get label() {
          return t`Switch Project`
        },
        icon: FolderOpen,
      },
      openCommandPalette: {
        get label() {
          return t`Open Command Menu`
        },
        icon: Command,
      },
      openKeyboardShortcuts: {
        get label() {
          return t`Open Keyboard Shortcuts`
        },
        icon: Keyboard,
      },
      openInbox: {
        get label() {
          return t`Open Inbox`
        },
        icon: Inbox,
      },
      openNextUnreadInboxItem: {
        get label() {
          return t`Open Next Unread Inbox Item`
        },
        icon: ListChecks,
      },
      openSessions: {
        get label() {
          return t`Open Sessions`
        },
        icon: MessageSquarePlus,
      },
      openSearch: {
        get label() {
          return t`Open Search`
        },
        icon: Search,
      },
      openSpecs: {
        get label() {
          return t`Open Specs`
        },
        icon: Folder,
      },
      openTasks: {
        get label() {
          return t`Open Tasks`
        },
        icon: ListTodo,
      },
      openRoadmap: {
        get label() {
          return t`Open Roadmap`
        },
        icon: Map,
      },
      openSettings: {
        get label() {
          return t`Open Settings`
        },
        icon: Settings,
      },
    },
  },
  inbox: {
    group: "navigation",
    commands: {
      selectUnreadFilter: {
        get label() {
          return t`Select Unread Inbox Filter`
        },
        icon: Inbox,
      },
      selectSavedFilter: {
        get label() {
          return t`Select Saved Inbox Filter`
        },
        icon: Inbox,
      },
      selectRepliedFilter: {
        get label() {
          return t`Select Replied Inbox Filter`
        },
        icon: Inbox,
      },
      selectCompletedFilter: {
        get label() {
          return t`Select Completed Inbox Filter`
        },
        icon: Inbox,
      },
      selectArchivedFilter: {
        get label() {
          return t`Select Archived Inbox Filter`
        },
        icon: Inbox,
      },
    },
  },
  projects: {
    group: "projects",
    commands: {
      openFolder: {
        get label() {
          return t`Open Folder`
        },
        icon: FolderOpen,
        get description() {
          return t`Open a project from your filesystem.`
        },
      },
    },
  },
  sessionInput: {
    group: "session",
    commands: {
      openProjectSelector: {
        get label() {
          return t`Open Project Selector`
        },
        icon: FolderOpen,
      },
      openSubpackageSelector: {
        get label() {
          return t`Open Working Directory Selector`
        },
        icon: Folder,
      },
      openAdapterSelector: {
        get label() {
          return t`Open Agent Harness Selector`
        },
        icon: Bot,
      },
      openLocationSelector: {
        get label() {
          return t`Cycle Launch Location`
        },
        icon: MapPin,
      },
      openBranchSelector: {
        get label() {
          return t`Open Branch Selector`
        },
        icon: GitBranch,
      },
      openApprovalPresetSelector: {
        get label() {
          return t`Open Approval Preset Selector`
        },
        icon: Brain,
      },
      openModelSelector: {
        get label() {
          return t`Open Model Selector`
        },
        icon: Brain,
      },
      openThinkingLevelSelector: {
        get label() {
          return t`Open Thinking Level Selector`
        },
        icon: Brain,
      },
      decreaseThinkingLevel: {
        get label() {
          return t`Decrease Thinking Level`
        },
        icon: Brain,
      },
      increaseThinkingLevel: {
        get label() {
          return t`Increase Thinking Level`
        },
        icon: Brain,
      },
      submit: {
        get label() {
          return t`Submit`
        },
        icon: SendHorizontal,
      },
    },
  },
  sessionChat: {
    group: "session",
    commands: {
      viewChanges: {
        get label() {
          return t`View Changes`
        },
        icon: FileDiff,
      },
      completeSession: {
        get label() {
          return t`Complete Session`
        },
        icon: CheckCircle2,
      },
      skipToPreviousPrompt: {
        get label() {
          return t`Skip to Previous Prompt`
        },
        icon: ChevronUp,
      },
      skipToNextPrompt: {
        get label() {
          return t`Skip to Next Prompt`
        },
        icon: ChevronDown,
      },
    },
  },
})
