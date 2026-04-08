import type { AppState } from "@/lib/types";
import { cn } from "@/lib/utils";

interface Props {
  state: AppState;
}

export function StatusBar({ state }: Props) {
  const activeMachine = state.machines.find((m) => m.active);
  const onlineCount = state.machines.filter((m) => m.online).length;

  return (
    <div className="h-7 flex items-center px-4 border-t border-border bg-surface-sunken text-[10px] text-text-tertiary shrink-0">
      <div className="flex items-center gap-4">
        <span className="flex items-center gap-1.5">
          <span
            className={cn(
              "w-1.5 h-1.5 rounded-full",
              state.deskflowStatus === "running" ? "bg-success" : "bg-danger"
            )}
          />
          deskflow {state.deskflowStatus}
        </span>
        <span>{onlineCount}/{state.machines.length} machines online</span>
        {activeMachine && <span>active: {activeMachine.name}</span>}
      </div>
      <div className="ml-auto flex items-center gap-3">
        <span>{state.monitors.length} monitor(s)</span>
        <span>v0.1.0</span>
      </div>
    </div>
  );
}
