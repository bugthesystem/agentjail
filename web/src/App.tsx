import { BrowserRouter, Routes, Route, Navigate } from "react-router-dom";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { AuthProvider, useAuth } from "./lib/auth";
import { Layout } from "./components/Layout";
import { LoginPage } from "./pages/Login";
import { DashboardPage } from "./pages/Dashboard";
import { SessionsPage } from "./pages/Sessions";
import { CredentialsPage } from "./pages/Credentials";
import { RunsPage } from "./pages/Runs";
import { AuditPage } from "./pages/Audit";

const queryClient = new QueryClient({
  defaultOptions: {
    queries: { staleTime: 2000, retry: 1 },
  },
});

function AuthGate() {
  const { auth } = useAuth();
  if (!auth) return <Navigate to="/login" replace />;
  return <Layout />;
}

function LoginGate() {
  const { auth } = useAuth();
  if (auth) return <Navigate to="/" replace />;
  return <LoginPage />;
}

export function App() {
  return (
    <QueryClientProvider client={queryClient}>
      <AuthProvider>
        <BrowserRouter>
          <Routes>
            <Route path="/login" element={<LoginGate />} />
            <Route element={<AuthGate />}>
              <Route index element={<DashboardPage />} />
              <Route path="sessions" element={<SessionsPage />} />
              <Route path="credentials" element={<CredentialsPage />} />
              <Route path="runs" element={<RunsPage />} />
              <Route path="audit" element={<AuditPage />} />
            </Route>
          </Routes>
        </BrowserRouter>
      </AuthProvider>
    </QueryClientProvider>
  );
}
