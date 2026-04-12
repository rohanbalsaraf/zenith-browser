/**
 * Type-safe IPC bridge for Zenith Browser
 */

interface ZenithWindow extends Window {
  zenithSetState?: (state: ChromeState) => void;
  zenithSetSuggestions?: (results: Suggestion[]) => void;
  ipc?: {
    postMessage: (message: string) => void;
  };
}

export interface Suggestion {
  title: string;
  url?: string;
  suggestionType: 'history' | 'bookmark' | 'tab';
  tabId?: number;
}

export interface ChromeTabState {
  id: number;
  title: string;
  url: string;
  isBookmarked: boolean;
  activePermissions: string[];
  isIncognito: boolean;
}

export interface ChromeState {
  tabs: ChromeTabState[];
  activeId: number | null;
}

export type IpcMessage = 
  | { type: 'chrome_ready' }
  | { type: 'new_tab', url?: string, activate: boolean, isIncognito?: boolean }
  | { type: 'switch_tab', tabId: number }
  | { type: 'close_tab', tabId: number }
  | { type: 'navigate', tabId?: number, url: string }
  | { type: 'tab_action', tabId?: number, action: 'reload' | 'back' | 'forward' }
  | { type: 'get_suggestions', query: string }
  | { type: 'bookmark_active_tab', tabId?: number }
  | { type: 'clear_history' }
  | { type: 'clear_downloads' }
  | { type: 'open_settings_tab' }
  | { type: 'open_history_tab' }
  | { type: 'open_downloads_tab' }
  | { type: 'settings-change', key: string, value: string };

class ZenithIpc {
  private listeners: Set<(state: ChromeState) => void> = new Set();
  private suggestionListeners: Set<(results: Suggestion[]) => void> = new Set();

  constructor() {
    // Listen for state updates from Rust
    (window as ZenithWindow).zenithSetState = (state: ChromeState) => {
      this.listeners.forEach(l => l(state));
    };

    // Listen for suggestion results
    (window as ZenithWindow).zenithSetSuggestions = (results: Suggestion[]) => {
      this.suggestionListeners.forEach(l => l(results));
    };
  }

  send(msg: IpcMessage) {
    const ipc = (window as ZenithWindow).ipc;
    if (ipc?.postMessage) {
      ipc.postMessage(JSON.stringify(msg));
    } else {
      console.warn('[IPC] Offline:', msg);
    }
  }

  onState(callback: (state: ChromeState) => void) {
    this.listeners.add(callback);
    return () => this.listeners.delete(callback);
  }

  onSuggestions(callback: (results: Suggestion[]) => void) {
    this.suggestionListeners.add(callback);
    return () => this.suggestionListeners.delete(callback);
  }
}

export const ipc = new ZenithIpc();
