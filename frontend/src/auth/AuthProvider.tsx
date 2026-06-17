import { useState, useEffect, useCallback, type ReactNode } from "react"
import { useLocation, useNavigate } from "react-router-dom"
import { api, setProjectHeader, ApiError, type ProjectInfo } from "@/api/client"
import { AuthContext, type AuthState } from "@/auth/AuthContext"

const ACTIVE_PROJECT_KEY = "mem_active_project"
const PROJECT_ROUTE_RE = /^\/([^/]+)\/(memories|changes|graph)(\/.*)?$/
const LEGACY_PROJECT_ROUTE_RE = /^\/(memories|changes|graph)(\/.*)?$/

function projectIdFromPath(pathname: string) {
  const match = pathname.match(PROJECT_ROUTE_RE)
  return match ? decodeURIComponent(match[1]) : ""
}

function projectPath(projectId: string, pathname: string) {
  const encodedProjectId = encodeURIComponent(projectId)
  const projectMatch = pathname.match(PROJECT_ROUTE_RE)
  if (projectMatch) return `/${encodedProjectId}/${projectMatch[2]}${projectMatch[3] || ""}`
  const legacyMatch = pathname.match(LEGACY_PROJECT_ROUTE_RE)
  if (legacyMatch) return `/${encodedProjectId}/${legacyMatch[1]}${legacyMatch[2] || ""}`
  return pathname
}

function searchWithoutLegacyProfile(search: string) {
  const params = new URLSearchParams(search)
  params.delete("profile")
  const nextSearch = params.toString()
  return nextSearch ? `?${nextSearch}` : ""
}

export function AuthProvider({ children }: { children: ReactNode }) {
  const [state, setState] = useState<AuthState>({ isLoggedIn: false, isLoading: true, projects: [], activeProject: null })
  const location = useLocation()
  const navigate = useNavigate()

  // checkSession 失败时抛出，由调用方决定如何处理
  const checkSession = useCallback(async () => {
    await api.auth.session();
    const projectsRes = await api.projects.list()
    const projects = projectsRes || []
    const urlProjectId = projectIdFromPath(window.location.pathname) || new URLSearchParams(window.location.search).get("profile")?.trim()
    const savedProjectId = localStorage.getItem(ACTIVE_PROJECT_KEY)
    // 有保存的项目或自动选第一个
    const preferredProjectId = urlProjectId || savedProjectId
    const activeProject = preferredProjectId
      ? projects.find(p => p.project_id === preferredProjectId) || projects[0] || null
      : projects[0] || null
    if (activeProject) {
      setProjectHeader(activeProject.project_id)
      localStorage.setItem(ACTIVE_PROJECT_KEY, activeProject.project_id)
    }
    setState({ isLoggedIn: true, isLoading: false, projects, activeProject })
  }, [])

  useEffect(() => {
    if (!state.activeProject) return
    const nextPath = projectPath(state.activeProject.project_id, location.pathname)
    const nextSearch = searchWithoutLegacyProfile(location.search)
    if (nextPath === location.pathname && nextSearch === location.search) return
    navigate(`${nextPath}${nextSearch}`, { replace: true })
  }, [state.activeProject, location.pathname, location.search, navigate])

  // 初始化：session 失效时只关 loading，保持未登录态
  useEffect(() => {
    let cancelled = false
    const timer = window.setTimeout(() => {
      checkSession().catch(() => {
        if (!cancelled) setState(s => ({ ...s, isLoading: false }))
      })
    }, 0)
    return () => {
      cancelled = true
      window.clearTimeout(timer)
    }
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
