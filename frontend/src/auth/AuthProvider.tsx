import { createContext, useContext, useState, useEffect, useCallback, type ReactNode } from "react"
import { api, setProjectHeader, ApiError, type ProjectInfo } from "@/api/client"

interface AuthState {
  isLoggedIn: boolean
  isLoading: boolean
  projects: ProjectInfo[]
  activeProject: ProjectInfo | null
}

interface AuthActions {
  login: (key: string) => Promise<{ success: boolean; error?: string }>
  logout: () => void
  selectProject: (project: ProjectInfo) => void
}

const AuthContext = createContext<(AuthState & AuthActions) | null>(null)

const ACTIVE_PROJECT_KEY = "mem_active_project"

export function AuthProvider({ children }: { children: ReactNode }) {
  const [state, setState] = useState<AuthState>({ isLoggedIn: false, isLoading: true, projects: [], activeProject: null })

  // checkSession 失败时抛出，由调用方决定如何处理
  const checkSession = useCallback(async () => {
    await api.auth.session();
    const projectsRes = await api.projects.list()
    const projects = projectsRes || []
    const savedProjectId = localStorage.getItem(ACTIVE_PROJECT_KEY)
    // 有保存的项目或自动选第一个
    const activeProject = savedProjectId
      ? projects.find(p => p.project_id === savedProjectId) || projects[0] || null
      : projects[0] || null
    if (activeProject) {
      setProjectHeader(activeProject.project_id)
      if (!savedProjectId) localStorage.setItem(ACTIVE_PROJECT_KEY, activeProject.project_id)
    }
    setState({ isLoggedIn: true, isLoading: false, projects, activeProject })
  }, [])

  // 初始化：session 失效时只关 loading，保持未登录态
  useEffect(() => {
    checkSession().catch(() => {
      setState(s => ({ ...s, isLoading: false }))
    })
  }, [checkSession])

  const login = async (key: string) => {
    try {
      await api.auth.verify(key);
      await checkSession();
      return { success: true }
    } catch (e) {
      const msg = e instanceof ApiError ? e.message : "认证失败"
      return { success: false, error: msg }
    }
  }

  const logout = () => {
    localStorage.removeItem(ACTIVE_PROJECT_KEY)
    setState({ isLoggedIn: false, isLoading: false, projects: [], activeProject: null })
    setProjectHeader("")
  }

  const selectProject = (project: ProjectInfo) => {
    localStorage.setItem(ACTIVE_PROJECT_KEY, project.project_id)
    setProjectHeader(project.project_id)
    setState(s => ({ ...s, activeProject: project }))
  }

  return (
    <AuthContext.Provider value={{ ...state, login, logout, selectProject }}>
      {children}
    </AuthContext.Provider>
  )
}

export function useAuth() {
  const ctx = useContext(AuthContext)
  if (!ctx) throw new Error("useAuth must be used within AuthProvider")
  return ctx
}