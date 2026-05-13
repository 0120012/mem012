import { Navigate, Outlet } from "react-router-dom"
import { useAuth } from "@/auth/AuthProvider"

export function ProjectGuard() {
  const { activeProject } = useAuth()
  if (!activeProject) return <Navigate to="/memories" replace />
  return <Outlet />
}