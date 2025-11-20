import { useState } from "react";
import { Link, useNavigate } from "react-router-dom";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Card } from "@/components/ui/card";
import logo from "@/assets/logo.png";
import { authAPI } from "@/lib/api";
import { setAuthToken, setUser } from "@/lib/auth";
import { toast } from "sonner";

const Login = () => {
  const [isu, setIsu] = useState("");
  const [password, setPassword] = useState("");
  const [loading, setLoading] = useState(false);
  const navigate = useNavigate();

  const handleSubmit = async (e: React.FormEvent) => {
    e.preventDefault();
    setLoading(true);
    
    try {
      const response = await authAPI.login({ isu, password });
      const { access_token, user } = response.data;
      
      // Сохраняем токен и пользователя
      setAuthToken(access_token);
      setUser(user);
      
      // Убеждаемся, что токен действительно записался
      const savedToken = localStorage.getItem('access_token');
      if (!savedToken || savedToken !== access_token) {
        throw new Error('Не удалось сохранить токен авторизации');
      }
      
      setLoading(false);
      toast.success("Вход выполнен успешно");
      

      if (user.role === "teacher" || user.role === "admin") {
        navigate("/teacher", { replace: true });
      } else {
        navigate("/student", { replace: true });
      }
    } catch (error: any) {
      toast.error(error.response?.data?.detail || error.message || "Ошибка входа");
      setLoading(false);
    }
  };

  return (
    <div className="min-h-screen flex items-center justify-center bg-gradient-subtle px-6">
      <Card className="w-full max-w-md p-8 shadow-elegant">
        <div className="flex flex-col items-center mb-8">
          <img src={logo} alt="Picrete" className="h-16 w-16 mb-4" />
          <h1 className="text-3xl font-bold bg-gradient-to-r from-primary to-accent bg-clip-text text-transparent">
            Вход в Picrete
          </h1>
          <p className="text-muted-foreground mt-2">Добро пожаловать обратно</p>
        </div>

        <form onSubmit={handleSubmit} className="space-y-6">
          <div className="space-y-2">
            <Label htmlFor="isu">Номер ИСУ</Label>
            <Input
              id="isu"
              type="text"
              placeholder="123456"
              value={isu}
              onChange={(e) => setIsu(e.target.value.replace(/\D/g, '').slice(0, 6))}
              required
              maxLength={6}
              pattern="\d{6}"
              className="transition-all duration-300 focus:shadow-soft"
            />
          </div>

          <div className="space-y-2">
            <Label htmlFor="password">Пароль</Label>
            <Input
              id="password"
              type="password"
              placeholder="••••••••"
              value={password}
              onChange={(e) => setPassword(e.target.value)}
              required
              className="transition-all duration-300 focus:shadow-soft"
            />
          </div>

          <Button type="submit" className="w-full" size="lg" disabled={loading}>
            {loading ? "Вход..." : "Войти"}
          </Button>
        </form>

        <div className="mt-6 text-center text-sm text-muted-foreground">
          Нет аккаунта?{" "}
          <Link to="/signup" className="text-primary hover:underline font-medium">
            Зарегистрироваться
          </Link>
        </div>

        <div className="mt-4 text-center">
          <Link to="/" className="text-sm text-muted-foreground hover:text-foreground transition-colors">
            ← Назад на главную
          </Link>
        </div>
      </Card>
    </div>
  );
};

export default Login;
