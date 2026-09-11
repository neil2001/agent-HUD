import { AgentList } from "./components/AgentList";
import { useSessions } from "./hooks/useSessions";
import "./styles.css";

function App() {
  const sessions = useSessions();

  if (sessions.length === 0) {
    return null;
  }

  return (
    <div className="hud-root">
      <div className="hud-drag-handle" data-tauri-drag-region />
      <AgentList sessions={sessions} />
    </div>
  );
}

export default App;
