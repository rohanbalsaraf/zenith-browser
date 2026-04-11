import React, { useState, useEffect } from 'react';
import { ipc } from './ipc';
import type { ChromeState, Suggestion } from './ipc';
import TabBar from './components/TabBar';
import Toolbar from './components/Toolbar';
import SuggestionsDropdown from './components/SuggestionsDropdown';
import PaletteSearch from './components/PaletteSearch';
import ErrorBoundary from './components/ErrorBoundary';

export default function App() {
  const [state, setState] = useState<ChromeState>({ tabs: [], activeId: null });
  const isPaletteOpen = (() => {
    const params = new URLSearchParams(window.location.search);
    return params.get('mode') === 'palette';
  })();
  const [searchQuery, setSearchQuery] = useState('');
  const [suggestions, setSuggestions] = useState<Suggestion[]>([]);
  const [selectedIndex, setSelectedIndex] = useState(-1);

  useEffect(() => {
    const unbindState = ipc.onState(setState);
    const unbindSuggestions = ipc.onSuggestions(setSuggestions);
    
    const handleKeyDown = (e: KeyboardEvent) => {
      if (suggestions.length > 0) {
        if (e.key === 'ArrowDown') {
          e.preventDefault();
          setSelectedIndex(prev => (prev + 1) % suggestions.length);
        } else if (e.key === 'ArrowUp') {
          e.preventDefault();
          setSelectedIndex(prev => (prev - 1 + suggestions.length) % suggestions.length);
        } else if (e.key === 'Enter' && selectedIndex >= 0) {
          e.preventDefault();
          const s = suggestions[selectedIndex];
          ipc.send({ type: 'navigate', tabId: state.activeId!, url: s.url || s.title });
          setSuggestions([]);
          setSelectedIndex(-1);
        } else if (e.key === 'Escape') {
          setSuggestions([]);
          setSelectedIndex(-1);
        }
      }
    };

    window.addEventListener('keydown', handleKeyDown);

    // Signal ready to Rust
    ipc.send({ type: 'chrome_ready' });

    return () => {
      unbindState();
      unbindSuggestions();
      window.removeEventListener('keydown', handleKeyDown);
    };
  }, [suggestions, selectedIndex, state.activeId]);

  const activeTab = state.tabs.find(t => t.id === state.activeId);

  return (
    <ErrorBoundary>
      <div className="flex flex-col h-screen overflow-hidden select-none bg-transparent">
        {/* Top Navigation & Tabs */}
        <div className="flex flex-col glass border-b border-zenith-border p-2 gap-2 h-[82px] justify-center">
          <TabBar tabs={state.tabs} activeId={state.activeId} />
          <Toolbar 
          activeId={state.activeId}
          activeTab={activeTab}
          searchQuery={searchQuery}
          onSearchChange={setSearchQuery}
          onSearchSubmit={(url) => {
            ipc.send({ type: 'navigate', tabId: state.activeId!, url });
          }}
        />
      </div>

      {/* Content Area (WebView Placeholder) */}
      <div className="flex-1 bg-transparent pointer-events-none" />

      {/* Suggestion Dropdown */}
      <SuggestionsDropdown 
        suggestions={suggestions} 
        selectedIndex={selectedIndex}
        activeId={state.activeId}
        onSuggestionsClose={() => {
          setSuggestions([]);
          setSelectedIndex(-1);
        }}
      />

      {/* Palette Mode */}
      {isPaletteOpen && (
        <PaletteSearch 
          searchQuery={searchQuery}
          onSearchChange={setSearchQuery}
        />
      )}
      </div>
    </ErrorBoundary>
  );
}
