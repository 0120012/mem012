import { createContext, useContext } from "react"
import type { ProjectInfo } from "@/api/client"

export interface AuthState {
  isLoggedIn: boolean
  isLoading: boolean
  projects: ProjectInfo[]
  activeProject: ProjectInfo | null
}

export interface AuthActions {
  login: (key: string) => Promise<{ success: boolean; error?: string }>
  logout: () => void
  selectProject: (project: ProjectInfo) => void
}

export const AuthContext = createContext<(AuthState & AuthActions) | null>(null)

export function useAuth() {
  const ctx = useContext(AuthContext)
  if (!ctx) throw new Error("useAuth must be used within AuthProvider")
  return ctx
}
