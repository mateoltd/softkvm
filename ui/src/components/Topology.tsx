import { useState } from "react";
import type { AppState, LayoutEntry, Machine } from "@/lib/types";
import { cn, osLabel } from "@/lib/utils";
import { Monitor, Plus, Trash2, ArrowRight } from "lucide-react";

interface Props {
  state: AppState;
}

function ScreenBlock({ entry, machine }: { entry: LayoutEntry; machine?: Machine }) {
  const isActive = machine?.active ?? false;
  const isOnline = machine?.online ?? false;

  return (
    <div
      className={cn(
        "relative rounded-lg border-2 transition-all cursor-move select-none",
        "flex flex-col items-center justify-center gap-1",
        isActive
          ? "border-accent bg-accent/5 shadow-sm"
          : isOnline
            ? "border-border bg-surface-raised hover:border-text-tertiary"
            : "border-border/50 bg-surface-sunken opacity-60"
      )}
      style={{ width: entry.width, height: entry.height }}
    >
      <Monitor size={20} className="text-text-tertiary" strokeWidth={1.5} />
      <span className="text-[12px] font-semibold text-text">{entry.machineName}</span>
      <span className="text-[10px] text-text-tertiary">
        {machine ? osLabel(machine.os) : "unknown"}
      </span>
      {isActive && (
        <div className="absolute -top-1 -right-1 w-3 h-3 rounded-full bg-success border-2 border-surface" />
      )}
    </div>
  );
}

export function Topology({ state }: Props) {
  const [layout] = useState(state.layout);

  return (
    <div>
      <h2 className="text-[15px] font-semibold text-text mb-1">Screen Layout</h2>
      <p className="text-[12px] text-text-tertiary mb-6">
        Arrange screens to match your physical desk setup. Drag to reorder.
      </p>

      {/* layout canvas */}
      <div className="bg-surface-sunken rounded-xl border border-border p-8 mb-8">
        <div className="flex items-center justify-center gap-4">
          {layout.map((entry) => {
            const machine = state.machines.find((m) => m.name === entry.machineName);
            return <ScreenBlock key={entry.machineName} entry={entry} machine={machine} />;
          })}
        </div>
        {layout.length >= 2 && (
          <div className="flex justify-center mt-4">
            <div className="flex items-center gap-2 text-[10px] text-text-tertiary">
              <span>{layout[0].machineName}</span>
              <ArrowRight size={12} />
              <span>{layout[1].machineName}</span>
            </div>
          </div>
        )}
      </div>

      {/* machine list */}
      <div className="mb-4 flex items-center justify-between">
        <h3 className="text-[13px] font-semibold text-text">Machines</h3>
        <button className="flex items-center gap-1.5 px-2.5 py-1.5 rounded-md text-[11px] font-medium text-text-secondary hover:text-text bg-surface-sunken border border-border hover:border-text-tertiary transition-colors">
          <Plus size={12} />
          Add Machine
        </button>
      </div>

      <div className="border border-border rounded-lg overflow-hidden">
        <table className="w-full text-[12px]">
          <thead>
            <tr className="bg-surface-sunken border-b border-border">
              <th className="text-left px-4 py-2 font-medium text-text-tertiary">Name</th>
              <th className="text-left px-4 py-2 font-medium text-text-tertiary">OS</th>
              <th className="text-left px-4 py-2 font-medium text-text-tertiary">Role</th>
              <th className="text-left px-4 py-2 font-medium text-text-tertiary">Status</th>
              <th className="w-10" />
            </tr>
          </thead>
          <tbody>
            {state.machines.map((m) => (
              <tr key={m.name} className="border-b border-border last:border-0 hover:bg-surface-sunken/50 transition-colors">
                <td className="px-4 py-2.5 font-medium text-text">{m.name}</td>
                <td className="px-4 py-2.5 text-text-secondary">{osLabel(m.os)}</td>
                <td className="px-4 py-2.5 text-text-secondary capitalize">{m.role}</td>
                <td className="px-4 py-2.5">
                  <span className="flex items-center gap-1.5">
                    <span
                      className={cn(
                        "w-1.5 h-1.5 rounded-full",
                        m.active ? "bg-success" : m.online ? "bg-text-tertiary" : "bg-danger"
                      )}
                    />
                    <span className="text-text-secondary">
                      {m.active ? "active" : m.online ? "online" : "offline"}
                    </span>
                  </span>
                </td>
                <td className="px-2 py-2.5">
                  <button className="p-1 rounded hover:bg-danger/10 text-text-tertiary hover:text-danger transition-colors">
                    <Trash2 size={12} />
                  </button>
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      </div>
    </div>
  );
}
