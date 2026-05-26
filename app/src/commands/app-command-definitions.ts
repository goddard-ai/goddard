import {
  Bot,
  Brain,
  ChevronDown,
  ChevronUp,
  Command,
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
    closeActiveTab: {
      label: "Close Active Tab",
      icon: PanelTopClose,
      when: "workbench.hasClosableActiveTab",
    },
  },
  navigation: {
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
  projects: {
    openFolder: {
      label: "Projects: Open Folder",
      icon: FolderOpen,
      description: "Open a project from your filesystem.",
    },
  },
  sessionInput: {
    openProjectSelector: {
      label: "Session Input: Open Project Selector",
      icon: FolderOpen,
    },
    openAdapterSelector: {
      label: "Session Input: Open Adapter Selector",
      icon: Bot,
    },
    openLocationSelector: {
      label: "Session Input: Open Launch Location Selector",
      icon: MapPin,
    },
    openBranchSelector: {
      label: "Session Input: Open Branch Selector",
      icon: GitBranch,
    },
    openModelSelector: {
      label: "Session Input: Open Model Selector",
      icon: Brain,
    },
    openThinkingLevelSelector: {
      label: "Session Input: Open Thinking Level Selector",
      icon: Brain,
    },
    submit: {
      label: "Session Input: Submit",
      icon: SendHorizontal,
    },
  },
  sessionChat: {
    skipToPreviousPrompt: {
      label: "Skip to Previous Prompt",
      icon: ChevronUp,
    },
    skipToNextPrompt: {
      label: "Skip to Next Prompt",
      icon: ChevronDown,
    },
  },
})
