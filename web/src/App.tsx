import { BrowserRouter, Routes, Route, Navigate } from "react-router-dom";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { AuthProvider, useAuth } from "./lib/auth";
import { Shell } from "./components/Shell";
import { Login } from "./components/Login";
import { Orbit } from "./components/Orbit";
import { Landing } from "./pages/Landing";
import { Overview } from "./pages/Overview";
import { Jails } from "./pages/Jails";
import { Sessions } from "./pages/Sessions";
import { Credentials } from "./pages/Credentials";
import { Stream } from "./pages/Stream";
import { Playground } from "./pages/Playground";
import { Settings } from "./pages/Settings";
import { Workspaces } from "./pages/Workspaces";
import { Snapshots } from "./pages/Snapshots";
import { DocsShell } from "./components/docs/DocsShell";
import { Quickstart } from "./pages/docs/Quickstart";
import { Sdk } from "./pages/docs/Sdk";
import { Phantom } from "./pages/docs/Phantom";
import { Network } from "./pages/docs/Network";
import { Forking } from "./pages/docs/Forking";
import { Security } from "./pages/docs/Security";
import { PlaygroundDoc } from "./pages/docs/PlaygroundDoc";
import { Workspaces as WorkspacesDoc } from "./pages/docs/Workspaces";
import { Snapshots as SnapshotsDoc } from "./pages/docs/Snapshots";
import { Gateway as GatewayDoc } from "./pages/docs/Gateway";

const qc = new QueryClient({
  defaultOptions: {
    queries: { refetchOnWindowFocus: false, retry: 1 },
  },
});

function Gate() {
  const { auth } = useAuth();
  return (
    <Routes>
      <Route path="/docs" element={<DocsShell />}>
        <Route index               element={<Navigate to="quickstart" replace />} />
        <Route path="quickstart"   element={<Quickstart />} />
        <Route path="sdk"          element={<Sdk />} />
        <Route path="playground"   element={<PlaygroundDoc />} />
        <Route path="phantom"      element={<Phantom />} />
        <Route path="network"      element={<Network />} />
        <Route path="forking"      element={<Forking />} />
        <Route path="workspaces"   element={<WorkspacesDoc />} />
        <Route path="snapshots"    element={<SnapshotsDoc />} />
        <Route path="gateway"      element={<GatewayDoc />} />
        <Route path="security"     element={<Security />} />
      </Route>
      {auth ? (
        <Route element={<Shell />}>
          <Route index                     element={<Overview />} />
          <Route path="projects"           element={<Workspaces />} />
          <Route path="sessions"           element={<Sessions />} />
          <Route path="integrations"       element={<Credentials />} />
          <Route path="playground"         element={<Playground />} />
          <Route path="operator/ledger"    element={<Jails />} />
          <Route path="operator/snapshots" element={<Snapshots />} />
          <Route path="operator/audit"     element={<Stream />} />
          <Route path="operator/settings"  element={<Settings />} />

          {/* Legacy paths — redirect so bookmarks keep working. */}
          <Route path="workspaces"  element={<Navigate to="/projects" replace />} />
          <Route path="credentials" element={<Navigate to="/integrations" replace />} />
          <Route path="jails"       element={<Navigate to="/operator/ledger" replace />} />
          <Route path="snapshots"   element={<Navigate to="/operator/snapshots" replace />} />
          <Route path="stream"      element={<Navigate to="/operator/audit" replace />} />
          <Route path="settings"    element={<Navigate to="/operator/settings" replace />} />

          <Route path="login"       element={<Navigate to="/" replace />} />
          <Route path="*"           element={<Navigate to="/" replace />} />
        </Route>
      ) : (
        <>
          <Route path="/"      element={<Landing />} />
          <Route path="/login" element={<Login />} />
          <Route path="*"      element={<Navigate to="/" replace />} />
        </>
      )}
    </Routes>
  );
}

export default function App() {
  return (
    <QueryClientProvider client={qc}>
      <AuthProvider>
        <Orbit />
        <BrowserRouter>
          <Gate />
        </BrowserRouter>
      </AuthProvider>
    </QueryClientProvider>
  );
}
