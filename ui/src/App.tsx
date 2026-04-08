import { useState } from "react";
import { mockState } from "./lib/mock-data";
import type { AppState } from "./lib/types";
import { TitleBar } from "./components/TitleBar";
import { Sidebar } from "./components/Sidebar";
import { StatusBar } from "./components/StatusBar";
import { Topology } from "./components/Topology";
import { Monitors } from "./components/Monitors";
import { Keyboard } from "./components/Keyboard";
import { Behavior } from "./components/Behavior";
import { Advanced } from "./components/Advanced";

type Tab = "topology" | "monitors" | "keyboard" | "behavior" | "advanced";

export function App() {
  const [state] = useState<AppState>(mockState);
  const [activeTab, setActiveTab] = useState<Tab>("topology");

  const panel = {
    topology: <Topology state={state} />,
    monitors: <Monitors state={state} />,
    keyboard: <Keyboard state={state} />,
    behavior: <Behavior state={state} />,
    advanced: <Advanced state={state} />,
  }[activeTab];

  return (
    <div className="h-full flex flex-col bg-surface">
      <TitleBar />
      <div className="flex flex-1 min-h-0">
        <Sidebar
          machines={state.machines}
          activeTab={activeTab}
          onTabChange={setActiveTab}
          focusLocked={state.focusLocked}
        />
        <main className="flex-1 min-w-0 overflow-y-auto">
          <div className="max-w-3xl mx-auto px-8 py-6">
            {panel}
          </div>
        </main>
      </div>
      <StatusBar state={state} />
    </div>
  );
}
