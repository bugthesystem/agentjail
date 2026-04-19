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

const qc = new QueryClient({
  defaultOptions: {
    queries: { refetchOnWindowFocus: false, retry: 1 },
  },
});

function Gate() {
  const { auth } = useAuth();
  return (
    <Routes>
      {auth ? (
        <Route element={<Shell />}>
          <Route index element={<Overview />} />
          <Route path="jails"       element={<Jails />} />
          <Route path="sessions"    element={<Sessions />} />
          <Route path="credentials" element={<Credentials />} />
          <Route path="stream"      element={<Stream />} />
          <Route path="playground"  element={<Playground />} />
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
