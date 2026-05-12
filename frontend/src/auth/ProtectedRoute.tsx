import { Navigate, Outlet } from "react-router-dom"
import { useAuth } from "@/auth/AuthProvider"

export function ProtectedRoute() {
  const { isLoggedIn, isLoading } = useAuth()
  if (isLoading) return <div className="flex h-screen items-center justify-center text-muted-foreground">加载中...</div>
  return isLoggedIn ? <Outlet /> : <Navigate to="/login" replace />
}