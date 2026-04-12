import { Plus, X, Ghost } from 'lucide-react';
import { motion, AnimatePresence } from 'framer-motion';
import { cn } from '../lib/utils';
import { ipc } from '../ipc';
import type { ChromeTabState } from '../ipc';

interface TabBarProps {
  tabs: ChromeTabState[];
  activeId: number | null;
}

export default function TabBar({ tabs, activeId }: TabBarProps) {
  return (
    <div className="flex items-center gap-2 overflow-x-auto no-scrollbar scroll-smooth">
      <AnimatePresence mode="popLayout">
        {tabs.map((tab) => (
          <motion.div
            key={tab.id}
            layout
            initial={{ opacity: 0, scale: 0.9, x: -10 }}
            animate={{ opacity: 1, scale: 1, x: 0 }}
            exit={{ opacity: 0, scale: 0.8, x: -20 }}
            onClick={() => ipc.send({ type: 'switch_tab', tabId: tab.id })}
            className={cn(
              "flex items-center gap-2 px-3 py-1.5 rounded-full text-xs font-medium cursor-pointer transition-all min-w-[120px] max-w-[200px] border border-transparent truncate whitespace-nowrap",
              tab.id === activeId 
                ? (tab.isIncognito ? "bg-purple-600/30 text-purple-100 border-purple-500/40 shadow-[0_0_10px_rgba(168,85,247,0.15)]" : "bg-white/10 text-white border-white/10") 
                : (tab.isIncognito ? "text-purple-300/60 hover:bg-purple-900/20 hover:text-purple-200" : "text-zenith-text-muted hover:bg-white/5 hover:text-white")
            )}
          >
            {tab.isIncognito && (
              <Ghost size={12} className={cn("shrink-0", tab.id === activeId ? "text-purple-300" : "text-purple-400/50")} />
            )}
            <span className="flex-1 truncate">{tab.title || 'New Tab'}</span>
            <button 
              onClick={(e) => {
                e.stopPropagation();
                ipc.send({ type: 'close_tab', tabId: tab.id });
              }}
              className="p-0.5 rounded-full hover:bg-white/20 transition-colors"
            >
              <X size={12} />
            </button>
          </motion.div>
        ))}
      </AnimatePresence>
      <button 
        onClick={() => ipc.send({ type: 'new_tab', activate: true })}
        className="p-2 rounded-full hover:bg-white/10 text-zenith-text-muted transition-colors"
      >
        <Plus size={16} />
      </button>
    </div>
  );
}
