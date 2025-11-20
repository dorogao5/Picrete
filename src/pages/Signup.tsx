import { useState } from "react";
import { Link, useNavigate } from "react-router-dom";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Card } from "@/components/ui/card";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select";
import logo from "@/assets/logo.png";
import { authAPI } from "@/lib/api";
import { setAuthToken, setUser } from "@/lib/auth";
import { toast } from "sonner";

const Signup = () => {
  const [isu, setIsu] = useState("");
  const [fullName, setFullName] = useState("");
  const [password, setPassword] = useState("");
  const [confirmPassword, setConfirmPassword] = useState("");
  const [role, setRole] = useState<"student" | "teacher">("student");
  const [loading, setLoading] = useState(false);
  const navigate = useNavigate();

  const handleSubmit = async (e: React.FormEvent) => {
    e.preventDefault();
    
    if (password !== confirmPassword) {
      toast.error("Пароли не совпадают");
      return;
    }

    if (isu.length !== 6) {
      toast.error("ISU должен содержать 6 цифр");
      return;
    }

    setLoading(true);
    try {
      const response = await authAPI.signup({
        isu,
        full_name: fullName,
        password,
        role,
      });
      
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
      toast.success("Регистрация успешна");
      
      // Навигация происходит после сохранения токена
      if (user.role === "teacher" || user.role === "admin") {
        navigate("/teacher", { replace: true });
      } else {
        navigate("/student", { replace: true });
      }
    } catch (error: any) {
      toast.error(error.response?.data?.detail || error.message || "Ошибка регистрации");
      setLoading(false);
    }
  };

  return (
    <div className="min-h-screen flex items-center justify-center bg-gradient-subtle px-6 py-12">
      <Card className="w-full max-w-md p-8 shadow-elegant">
        <div className="flex flex-col items-center mb-8">
          <img src={logo} alt="Picrete" className="h-16 w-16 mb-4" />
          <h1 className="text-3xl font-bold bg-gradient-to-r from-primary to-accent bg-clip-text text-transparent">
            Регистрация
          </h1>
          <p className="text-muted-foreground mt-2">Создайте аккаунт Picrete</p>
        </div>

        <form onSubmit={handleSubmit} className="space-y-6">
          <div className="space-y-2">
            <Label htmlFor="fullName">Полное имя *</Label>
            <Input
              id="fullName"
              type="text"
              placeholder="Иванов Иван Иванович"
              value={fullName}
              onChange={(e) => setFullName(e.target.value)}
              required
              className="transition-all duration-300 focus:shadow-soft"
            />
          </div>

          <div className="space-y-2">
            <Label htmlFor="isu">Номер ИСУ *</Label>
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
            <Label htmlFor="password">Пароль *</Label>
            <Input
              id="password"
              type="password"
              placeholder="••••••••"
              value={password}
              onChange={(e) => setPassword(e.target.value)}
              required
              minLength={6}
              className="transition-all duration-300 focus:shadow-soft"
            />
          </div>

          <div className="space-y-2">
            <Label htmlFor="confirmPassword">Подтвердите пароль *</Label>
            <Input
              id="confirmPassword"
              type="password"
              placeholder="••••••••"
              value={confirmPassword}
              onChange={(e) => setConfirmPassword(e.target.value)}
              required
              minLength={6}
              className="transition-all duration-300 focus:shadow-soft"
            />
          </div>

          <div className="space-y-2">
            <Label htmlFor="role">Роль *</Label>
            <Select value={role} onValueChange={(value) => setRole(value as "student" | "teacher")}>
              <SelectTrigger>
                <SelectValue placeholder="Выберите роль" />
              </SelectTrigger>
              <SelectContent>
                <SelectItem value="student">Студент</SelectItem>
                <SelectItem value="teacher">Преподаватель</SelectItem>
              </SelectContent>
            </Select>
          </div>

          <Button type="submit" className="w-full" size="lg" disabled={loading}>
            {loading ? "Регистрация..." : "Создать аккаунт"}
          </Button>
        </form>

        <div className="mt-6 text-center text-sm text-muted-foreground">
          Уже есть аккаунт?{" "}
          <Link to="/login" className="text-primary hover:underline font-medium">
            Войти
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

export default Signup;
