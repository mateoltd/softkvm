import type { AppState } from "@/lib/types";
import { cn } from "@/lib/utils";
import { RefreshCw, Zap, CircleCheck, CircleAlert, CircleX } from "lucide-react";

interface Props {
  state: AppState;
}

function HealthBadge({ health }: { health: string }) {
  const config = {
    healthy: { icon: CircleCheck, color: "text-success", label: "Healthy" },
    degraded: { icon: CircleAlert, color: "text-warning", label: "Degraded" },
    unavailable: { icon: CircleX, color: "text-danger", label: "Unavailable" },
  }[health] ?? { icon: CircleX, color: "text-text-tertiary", label: health };

  const Icon = config.icon;

  return (
    <span className={cn("flex items-center gap-1", config.color)}>
      <Icon size={12} />
      <span className="text-[11px] font-medium">{config.label}</span>
    </span>
  );
}

export function Monitors({ state }: Props) {
  return (
    <div>
      <div className="flex items-center justify-between mb-1">
        <h2 className="text-[15px] font-semibold text-text">Monitors</h2>
        <button className="flex items-center gap-1.5 px-2.5 py-1.5 rounded-md text-[11px] font-medium text-text-secondary hover:text-text bg-surface-sunken border border-border hover:border-text-tertiary transition-colors">
          <RefreshCw size={12} />
          Rescan
        </button>
      </div>
      <p className="text-[12px] text-text-tertiary mb-6">
        Detected monitors and their DDC/CI input source mappings.
      </p>

      {state.monitors.length === 0 ? (
        <div className="text-center py-16 text-text-tertiary">
          <p className="text-[13px] font-medium mb-1">No monitors detected</p>
          <p className="text-[11px]">Run a rescan or check DDC/CI support on your displays.</p>
        </div>
      ) : (
        <div className="space-y-4">
          {state.monitors.map((mon) => (
            <div
              key={mon.id}
              className="border border-border rounded-lg overflow-hidden"
            >
              {/* monitor header */}
              <div className="flex items-center justify-between px-4 py-3 bg-surface-sunken border-b border-border">
                <div>
                  <div className="text-[13px] font-semibold text-text">{mon.name}</div>
                  <div className="text-[10px] text-text-tertiary font-mono mt-0.5">{mon.id}</div>
                </div>
                <div className="flex items-center gap-3">
                  <HealthBadge health={mon.ddcHealth} />
                  <span className="text-[10px] text-text-tertiary px-2 py-0.5 bg-surface rounded border border-border">
                    {mon.connectionType}
                  </span>
                </div>
              </div>

              {/* input mappings */}
              <div className="px-4 py-3">
                <div className="text-[10px] font-semibold tracking-widest text-text-tertiary uppercase mb-2">
                  Input Mapping
                </div>
                <div className="space-y-2">
                  {Object.entries(mon.inputs).map(([machine, input]) => {
                    const isCurrent = input === mon.currentInput;
                    return (
                      <div
                        key={machine}
                        className={cn(
                          "flex items-center justify-between px-3 py-2 rounded-md border transition-colors",
                          isCurrent
                            ? "border-accent/30 bg-accent/5"
                            : "border-border bg-surface"
                        )}
                      >
                        <div className="flex items-center gap-2">
                          <span
                            className={cn(
                              "w-1.5 h-1.5 rounded-full",
                              isCurrent ? "bg-success" : "bg-border"
                            )}
                          />
                          <span className="text-[12px] font-medium text-text">{machine}</span>
                        </div>
                        <div className="flex items-center gap-2">
                          <span className="text-[11px] text-text-secondary font-mono">{input}</span>
                          {!isCurrent && (
                            <button
                              className="flex items-center gap-1 px-2 py-0.5 rounded text-[10px] font-medium text-text-tertiary hover:text-accent bg-surface-sunken border border-border hover:border-accent/30 transition-colors"
                              title={`Test switch to ${input}`}
                            >
                              <Zap size={10} />
                              Test
                            </button>
                          )}
                          {isCurrent && (
                            <span className="text-[10px] font-medium text-success">current</span>
                          )}
                        </div>
                      </div>
                    );
                  })}
                </div>
              </div>
            </div>
          ))}
        </div>
      )}
    </div>
  );
}
