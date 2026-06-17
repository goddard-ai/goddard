import {
  Bot,
  Brain,
  CheckCircle2,
  ChevronDown,
  ChevronUp,
  Command,
  FileDiff,
  Folder,
  FolderOpen,
  GitBranch,
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
} from "lucide-react"

import { text } from "~/language/text.ts"
import { defineAppCommands } from "./app-command.ts"

/** App command definitions surfaced to shortcut bindings and the command menu. */
export const AppCommand = defineAppCommands({
  workbench: {
    group: "workbench",
    commands: {
      closeActiveTab: {
        label: text.closeActiveTab,
        icon: PanelTopClose,
        when: "workbench.hasClosableActiveTab",
      },
    },
  },
  navigation: {
    group: "navigation",
    commands: {
      openProposeTaskDialog: {
        label: text.openProposeTaskDialog,
        icon: Lightbulb,
      },
      openNewSessionDialog: {
        label: text.openNewSessionDialog,
        icon: MessageSquarePlus,
      },
      openSwitchProject: {
        label: text.switchProject,
        icon: FolderOpen,
      },
      openCommandPalette: {
        label: text.openCommandMenu,
        icon: Command,
      },
      openKeyboardShortcuts: {
        label: text.openKeyboardShortcuts,
        icon: Keyboard,
      },
      openInbox: {
        label: text.openInbox,
        icon: Inbox,
      },
      openNextUnreadInboxItem: {
        label: text.openNextUnreadInboxItemTitle,
        icon: ListChecks,
      },
      openSessions: {
        label: text.openSessions,
        icon: MessageSquarePlus,
      },
      openSearch: {
        label: text.openSearch,
        icon: Search,
      },
      openSpecs: {
        label: text.openSpecs,
        icon: Folder,
      },
      openTasks: {
        label: text.openTasks,
        icon: ListTodo,
      },
      openRoadmap: {
        label: text.openRoadmap,
        icon: Map,
      },
      openSettings: {
        label: text.openSettings,
        icon: Settings,
      },
    },
  },
  inbox: {
    group: "navigation",
    commands: {
      selectUnreadFilter: {
        label: text.selectUnreadInboxFilter,
        icon: Inbox,
      },
      selectSavedFilter: {
        label: text.selectSavedInboxFilter,
        icon: Inbox,
      },
      selectRepliedFilter: {
        label: text.selectRepliedInboxFilter,
        icon: Inbox,
      },
      selectCompletedFilter: {
        label: text.selectCompletedInboxFilter,
        icon: Inbox,
      },
      selectArchivedFilter: {
        label: text.selectArchivedInboxFilter,
        icon: Inbox,
      },
    },
  },
  projects: {
    group: "projects",
    commands: {
      openFolder: {
        label: text.openFolder,
        icon: FolderOpen,
        description: text.openAProjectFromYourFilesystem,
      },
    },
  },
  sessionInput: {
    group: "session",
    commands: {
      openProjectSelector: {
        label: text.openProjectSelector,
        icon: FolderOpen,
      },
      openSubpackageSelector: {
        label: text.openSubpackageSelector,
        icon: Folder,
      },
      openAdapterSelector: {
        label: text.openAgentHarnessSelector,
        icon: Bot,
      },
      openLocationSelector: {
        label: text.cycleLaunchLocation,
        icon: MapPin,
      },
      openBranchSelector: {
        label: text.openBranchSelector,
        icon: GitBranch,
      },
      openApprovalPresetSelector: {
        label: text.openApprovalPresetSelector,
        icon: Brain,
      },
      openModelSelector: {
        label: text.openModelSelector,
        icon: Brain,
      },
      openThinkingLevelSelector: {
        label: text.openThinkingLevelSelector,
        icon: Brain,
      },
      decreaseThinkingLevel: {
        label: text.decreaseThinkingLevel,
        icon: Brain,
      },
      increaseThinkingLevel: {
        label: text.increaseThinkingLevel,
        icon: Brain,
      },
      submit: {
        label: text.submit,
        icon: SendHorizontal,
      },
    },
  },
  sessionChat: {
    group: "session",
    commands: {
      viewChanges: {
        label: text.viewChanges,
        icon: FileDiff,
      },
      completeSession: {
        label: text.completeSession,
        icon: CheckCircle2,
      },
      skipToPreviousPrompt: {
        label: text.skipToPreviousPrompt,
        icon: ChevronUp,
      },
      skipToNextPrompt: {
        label: text.skipToNextPrompt,
        icon: ChevronDown,
      },
    },
  },
})
