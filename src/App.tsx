import { AgentList } from "./components/AgentList";
import { DailyStrip } from "./components/DailyStrip";
import { useDailyUsage } from "./hooks/useDailyUsage";
import { useSessions } from "./hooks/useSessions";
import "./styles.css";

function App() {
  const sessions = useSessions();
  const usage = useDailyUsage();

  return (
    <div className="hud-root">
      <div className="hud-drag-handle" data-tauri-drag-region />
      <DailyStrip usage={usage} />
      {sessions.length > 0 ? <AgentList sessions={sessions} /> : null}
    </div>
  );
}

export default App;
