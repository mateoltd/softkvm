import {
  LayoutGrid,
  Monitor,
  Keyboard as KeyboardIcon,
  SlidersHorizontal,
  Terminal,
  Lock,
  LockOpen,
} from "lucide-react";
import type { Machine } from "@/lib/types";
import { cn, osLabel } from "@/lib/utils";

type Tab = "topology" | "monitors" | "keyboard" | "behavior" | "advanced";

interface Props {
  machines: Machine[];
  activeTab: Tab;
  onTabChange: (tab: Tab) => void;
  focusLocked: boolean;
}

const tabs: { id: Tab; label: string; icon: typeof LayoutGrid }[] = [
  { id: "topology", label: "Topology", icon: LayoutGrid },
  { id: "monitors", label: "Monitors", icon: Monitor },
  { id: "keyboard", label: "Keyboard", icon: KeyboardIcon },
  { id: "behavior", label: "Behavior", icon: SlidersHorizontal },
  { id: "advanced", label: "Advanced", icon: Terminal },
];

export function Sidebar({ machines, activeTab, onTabChange, focusLocked }: Props) {
  return (
    <aside className="w-52 shrink-0 border-r border-border bg-surface-sunken flex flex-col">
      {/* machine status */}
      <div className="px-4 pt-5 pb-3">
        <div className="text-[10px] font-semibold tracking-widest text-text-tertiary uppercase mb-3">
          Machines
        </div>
        <div className="space-y-1.5">
          {machines.map((m) => (
            <div
              key={m.name}
              className="flex items-center gap-2.5 px-2 py-1.5 rounded-md"
            >
              <div
                className={cn(
                  "w-1.5 h-1.5 rounded-full shrink-0",
                  m.active ? "bg-success" : m.online ? "bg-text-tertiary" : "bg-danger"
                )}
              />
              <div className="min-w-0 flex-1">
                <div className="text-[12px] font-medium text-text truncate leading-tight">
                  {m.name}
                </div>
                <div className="text-[10px] text-text-tertiary leading-tight">
                  {osLabel(m.os)} {m.active ? "— active" : m.online ? "— online" : "— offline"}
                </div>
              </div>
            </div>
          ))}
        </div>
      </div>

      {/* focus lock indicator */}
      <div className="px-4 pb-3">
        <button
          className={cn(
            "w-full flex items-center gap-2 px-2.5 py-1.5 rounded-md text-[11px] font-medium transition-colors",
            focusLocked
              ? "bg-warning/10 text-warning"
              : "text-text-tertiary hover:text-text-secondary hover:bg-surface"
          )}
        >
          {focusLocked ? <Lock size={12} /> : <LockOpen size={12} />}
          Focus {focusLocked ? "Locked" : "Unlocked"}
        </button>
      </div>

      <div className="h-px bg-border mx-4" />

      {/* navigation */}
      <nav className="flex-1 px-3 py-3 space-y-0.5 no-drag">
        {tabs.map(({ id, label, icon: Icon }) => (
          <button
            key={id}
            onClick={() => onTabChange(id)}
            className={cn(
              "w-full flex items-center gap-2.5 px-2.5 py-2 rounded-md text-[12px] font-medium transition-all",
              activeTab === id
                ? "bg-accent text-surface"
                : "text-text-secondary hover:text-text hover:bg-surface"
            )}
          >
            <Icon size={14} strokeWidth={activeTab === id ? 2 : 1.5} />
            {label}
          </button>
        ))}
      </nav>
    </aside>
  );
}
