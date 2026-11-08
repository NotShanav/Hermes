import { Lock } from "lucide-react";
import { AppProvider, useApp } from "./store";
import { Sidebar, Topbar, Toast } from "./components/Shell";
import LockScreen from "./components/LockScreen";
import Overview from "./pages/Overview";
import Timeline from "./pages/Timeline";
import Applications from "./pages/Applications";
import PagesWindows from "./pages/PagesWindows";
import Insights from "./pages/Insights";
import Explore from "./pages/Explore";
import Categories from "./pages/Categories";
import Privacy from "./pages/Privacy";
import Archive from "./pages/Archive";
import SettingsPage from "./pages/Settings";

const PAGES = {
  overview: Overview,
  timeline: Timeline,
  apps: Applications,
  pages: PagesWindows,
  insights: Insights,
  explore: Explore,
  categories: Categories,
  privacy: Privacy,
  archive: Archive,
  settings: SettingsPage,
};

function LockFlash() {
  const { lockFlash } = useApp();
  if (!lockFlash) return null;
  return (
    <div className="lock-flash" role="alert" aria-live="assertive">
      <div className="lock-flash-icon"><Lock size={44} strokeWidth={1.8} /></div>
      <b>Locked</b>
      <span className="muted">USB key removed</span>
    </div>
  );
}

function Root() {
  const { info, bootError, route } = useApp();
  if (bootError) {
    return (
      <div className="boot">
        <h2>Hermes could not start</h2>
        <p className="pre">{bootError}</p>
      </div>
    );
  }
  if (!info) return <div className="boot"><span className="spin" /></div>;
  if (!info.unlocked) return (<><LockScreen /><Toast /><LockFlash /></>);
  const Page = PAGES[route.page] || Overview;
  return (
    <div className="app">
      <Sidebar />
      <div className="main">
        <Topbar />
        <main className="page" key={route.page + (route.param || "")}>
          <Page />
        </main>
      </div>
      <Toast />
      <LockFlash />
    </div>
  );
}

export default function App() {
  return (
    <AppProvider>
      <Root />
    </AppProvider>
  );
}
