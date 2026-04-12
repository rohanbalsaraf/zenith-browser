import React from 'react';
import { ChevronLeft, ChevronRight, RotateCw, Settings, History, Download, Bookmark, Search, Ghost } from 'lucide-react';
import { cn } from '../lib/utils';
import { ipc } from '../ipc';
import type { ChromeTabState } from '../ipc';

interface ToolbarProps {
  activeId: number | null;
  activeTab: ChromeTabState | undefined;
  searchQuery: string;
  onSearchChange: (query: string) => void;
  onSearchSubmit: (url: string) => void;
}

export default function Toolbar({
  activeId,
  activeTab,
  searchQuery,
  onSearchChange,
  onSearchSubmit,
}: ToolbarProps) {
  return (
    <div className="flex items-center gap-3 px-1">
      <div className="flex items-center gap-1">
        <NavButton 
          icon={<ChevronLeft size={18} />} 
          onClick={() => ipc.send({ type: 'tab_action', tabId: activeId!, action: 'back' })} 
          disabled={!activeId} 
        />
        <NavButton 
          icon={<ChevronRight size={18} />} 
          onClick={() => ipc.send({ type: 'tab_action', tabId: activeId!, action: 'forward' })} 
          disabled={!activeId} 
        />
        <NavButton 
          icon={<RotateCw size={17} />} 
          onClick={() => ipc.send({ type: 'tab_action', tabId: activeId!, action: 'reload' })} 
          disabled={!activeId} 
        />
      </div>

      <div className="flex-1 relative group">
        <div className="absolute left-3 top-1/2 -translate-y-1/2 text-zenith-text-muted group-focus-within:text-zenith-primary transition-colors">
          {activeTab?.isIncognito ? <Ghost size={14} className="text-purple-400" /> : <Search size={14} />}
        </div>
        <input 
          className={cn(
            "w-full glass rounded-full py-1.5 pl-9 pr-10 text-sm focus:outline-none focus:border-zenith-primary/50 focus:bg-white/10 transition-all placeholder:text-zenith-text-muted",
            activeTab?.isIncognito && "border-purple-500/20 focus:border-purple-500/50"
          )}
          placeholder="Search or enter URL"
          value={searchQuery || (activeTab?.url || '')}
          onChange={(e) => onSearchChange(e.target.value)}
          onFocus={(e) => e.target.select()}
          onKeyDown={(e) => {
            if (e.key === 'Enter') {
              onSearchSubmit(e.currentTarget.value);
              e.currentTarget.blur();
            }
          }}
        />
        {activeTab && (
          <button 
            onClick={() => ipc.send({ type: 'bookmark_active_tab', tabId: activeTab.id })}
            className={cn(
              "absolute right-3 top-1/2 -translate-y-1/2 transition-colors hover:text-white",
              activeTab.isBookmarked ? "text-yellow-400" : "text-zenith-text-muted"
            )}
          >
            <Bookmark size={14} fill={activeTab.isBookmarked ? "currentColor" : "none"} />
          </button>
        )}
      </div>

      <div className="flex items-center gap-1">
        <NavButton icon={<History size={17} />} onClick={() => ipc.send({ type: 'open_history_tab' })} />
        <NavButton icon={<Download size={17} />} onClick={() => ipc.send({ type: 'open_downloads_tab' })} />
        <NavButton icon={<Settings size={17} />} onClick={() => ipc.send({ type: 'open_settings_tab' })} />
      </div>
    </div>
  );
}

function NavButton({ icon, onClick, disabled }: { icon: React.ReactNode, onClick: () => void, disabled?: boolean }) {
  return (
    <button 
      onClick={onClick}
      disabled={disabled}
      className={cn(
        "p-2 rounded-lg transition-colors",
        disabled ? "opacity-30 cursor-not-allowed" : "hover:bg-white/10 text-zenith-text-muted hover:text-white"
      )}
    >
      {icon}
    </button>
  );
}
