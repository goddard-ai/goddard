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
  ListTodo,
  Map,
  MapPin,
  MessageSquarePlus,
  PanelTopClose,
  Search,
  SendHorizontal,
  Settings,
} from "lucide-react"

import { defineAppCommands } from "./app-command.ts"

/** App command definitions surfaced to shortcut bindings and the command menu. */
export const AppCommand = defineAppCommands({
  workbench: {
    group: "workbench",
    commands: {
      closeActiveTab: {
        label: "Close Active Tab",
        icon: PanelTopClose,
        when: "workbench.hasClosableActiveTab",
      },
    },
  },
  navigation: {
    group: "navigation",
    commands: {
      openProposeTaskDialog: {
        label: "Open Propose Task Dialog",
        icon: Lightbulb,
      },
      openNewSessionDialog: {
        label: "Open New Session Dialog",
        icon: MessageSquarePlus,
      },
      openSwitchProject: {
        label: "Switch Project",
        icon: FolderOpen,
      },
      openCommandPalette: {
        label: "Open Command Menu",
        icon: Command,
      },
      openKeyboardShortcuts: {
        label: "Open Keyboard Shortcuts",
        icon: Keyboard,
      },
      openInbox: {
        label: "Open Inbox",
        icon: Inbox,
      },
      openSessions: {
        label: "Open Sessions",
        icon: MessageSquarePlus,
      },
      openSearch: {
        label: "Open Search",
        icon: Search,
      },
      openSpecs: {
        label: "Open Specs",
        icon: Folder,
      },
      openTasks: {
        label: "Open Tasks",
        icon: ListTodo,
      },
      openRoadmap: {
        label: "Open Roadmap",
        icon: Map,
      },
      openSettings: {
        label: "Open Settings",
        icon: Settings,
      },
    },
  },
  inbox: {
    group: "navigation",
    commands: {
      selectUnreadFilter: {
        label: "Select Unread Inbox Filter",
        icon: Inbox,
      },
      selectSavedFilter: {
        label: "Select Saved Inbox Filter",
        icon: Inbox,
      },
      selectRepliedFilter: {
        label: "Select Replied Inbox Filter",
        icon: Inbox,
      },
      selectCompletedFilter: {
        label: "Select Completed Inbox Filter",
        icon: Inbox,
      },
      selectArchivedFilter: {
        label: "Select Archived Inbox Filter",
        icon: Inbox,
      },
    },
  },
  projects: {
    group: "projects",
    commands: {
      openFolder: {
        label: "Open Folder",
        icon: FolderOpen,
        description: "Open a project from your filesystem.",
      },
    },
  },
  sessionInput: {
    group: "session",
    commands: {
      openProjectSelector: {
        label: "Open Project Selector",
        icon: FolderOpen,
      },
      openSubpackageSelector: {
        label: "Open Working Directory Selector",
        icon: Folder,
      },
      openAdapterSelector: {
        label: "Open Adapter Selector",
        icon: Bot,
      },
      openLocationSelector: {
        label: "Cycle Launch Location",
        icon: MapPin,
      },
      openBranchSelector: {
        label: "Open Branch Selector",
        icon: GitBranch,
      },
      openApprovalPresetSelector: {
        label: "Open Approval Preset Selector",
        icon: Brain,
      },
      openModelSelector: {
        label: "Open Model Selector",
        icon: Brain,
      },
      openThinkingLevelSelector: {
        label: "Open Thinking Level Selector",
        icon: Brain,
      },
      submit: {
        label: "Submit",
        icon: SendHorizontal,
      },
    },
  },
  sessionChat: {
    group: "session",
    commands: {
      viewChanges: {
        label: "View Changes",
        icon: FileDiff,
      },
      completeSession: {
        label: "Complete Session",
        icon: CheckCircle2,
      },
      skipToPreviousPrompt: {
        label: "Skip to Previous Prompt",
        icon: ChevronUp,
      },
      skipToNextPrompt: {
        label: "Skip to Next Prompt",
        icon: ChevronDown,
      },
    },
  },
})
