import { createContext, use, useState, useCallback, type ReactNode } from "react";
import { createApi, type Api } from "./api";

interface AuthState {
  apiKey: string;
  baseUrl: string;
  api: Api;
}

interface AuthContext {
  auth: AuthState | null;
  login: (baseUrl: string, apiKey: string) => Promise<void>;
  logout: () => void;
  isLoading: boolean;
  error: string | null;
}

const Ctx = createContext<AuthContext | null>(null);

const STORAGE_KEY = "agentjail_auth";

function loadSaved(): { baseUrl: string; apiKey: string } | null {
  try {
    const raw = localStorage.getItem(STORAGE_KEY);
    if (!raw) return null;
    return JSON.parse(raw);
  } catch {
    return null;
  }
}

export function AuthProvider({ children }: { children: ReactNode }) {
  const [auth, setAuth] = useState<AuthState | null>(() => {
    const saved = loadSaved();
    if (!saved) return null;
    return { ...saved, api: createApi(saved.baseUrl, saved.apiKey) };
  });
  const [isLoading, setIsLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const login = useCallback(async (baseUrl: string, apiKey: string) => {
    setIsLoading(true);
    setError(null);
    try {
      const url = baseUrl.replace(/\/+$/, "");
      const api = createApi(url, apiKey);
      await api.health();
      localStorage.setItem(STORAGE_KEY, JSON.stringify({ baseUrl: url, apiKey }));
      setAuth({ baseUrl: url, apiKey, api });
    } catch (e) {
      setError(e instanceof Error ? e.message : "Connection failed");
      throw e;
    } finally {
      setIsLoading(false);
    }
  }, []);

  const logout = useCallback(() => {
    localStorage.removeItem(STORAGE_KEY);
    setAuth(null);
  }, []);

  return (
    <Ctx value={{ auth, login, logout, isLoading, error }}>
      {children}
    </Ctx>
  );
}

export function useAuth() {
  const ctx = use(Ctx);
  if (!ctx) throw new Error("useAuth must be used within AuthProvider");
  return ctx;
}

export function useApi(): Api {
  const { auth } = useAuth();
  if (!auth) throw new Error("Not authenticated");
  return auth.api;
}
