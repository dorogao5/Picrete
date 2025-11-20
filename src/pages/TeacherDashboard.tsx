import { useState, useEffect } from "react";
import { Link } from "react-router-dom";
import { Button } from "@/components/ui/button";
import { Card } from "@/components/ui/card";
import { Navbar } from "@/components/Navbar";
import { Plus, FileText, CheckCircle, Clock, AlertCircle } from "lucide-react";
import { examsAPI } from "@/lib/api";
import { toast } from "sonner";

interface ExamSummary {
  id: string;
  title: string;
  start_time: string;
  end_time: string;
  duration_minutes: number;
  status: string;
  task_count: number;
  student_count: number;
  pending_count: number;
}

const TeacherDashboard = () => {
  const [exams, setExams] = useState<ExamSummary[]>([]);
  const [loading, setLoading] = useState(true);

  useEffect(() => {
    const fetchExams = async () => {
      // Проверяем наличие токена перед запросами
      const token = localStorage.getItem('access_token');
      if (!token) {
        // Если токена нет, не делаем запросы - interceptor обработает редирект
        setLoading(false);
        return;
      }
      
      try {
        const response = await examsAPI.list();
        setExams(response.data);
      } catch (error: any) {
        console.error("Error fetching exams:", error);
        // Не показываем ошибку для 401 - interceptor сам обработает редирект
        if (error.response?.status === 401) {
          setLoading(false);
          return;
        }
        // Для других ошибок показываем уведомление
        toast.error("Ошибка загрузки контрольных работ");
      } finally {
        setLoading(false);
      }
    };

    fetchExams();
  }, []);

  // Calculate statistics
  const stats = {
    total: exams.length,
    active: exams.filter(e => e.status === 'active' || e.status === 'published').length,
    pendingReview: exams.reduce((sum, e) => sum + e.pending_count, 0),
    completed: exams.reduce((sum, e) => sum + (e.student_count - e.pending_count), 0),
  };

  return (
    <div className="min-h-screen bg-gradient-subtle">
      <Navbar />
      
      <div className="container mx-auto px-6 pt-24 pb-12">
        <div className="flex items-center justify-between mb-8">
          <div>
            <h1 className="text-4xl font-bold mb-2">Панель преподавателя</h1>
            <p className="text-muted-foreground">Управление контрольными работами и проверка решений</p>
          </div>
          <Link to="/create-exam">
            <Button size="lg" className="shadow-elegant">
              <Plus className="w-5 h-5 mr-2" />
              Создать КР
            </Button>
          </Link>
        </div>

        {/* Stats Overview */}
        <div className="grid md:grid-cols-4 gap-6 mb-8">
          <Card className="p-6 bg-gradient-card border-border/50">
            <div className="flex items-center gap-4">
              <div className="w-12 h-12 rounded-xl bg-gradient-to-br from-primary to-accent flex items-center justify-center">
                <FileText className="w-6 h-6 text-white" />
              </div>
              <div>
                <p className="text-2xl font-bold">{loading ? "..." : stats.total}</p>
                <p className="text-sm text-muted-foreground">Всего КР</p>
              </div>
            </div>
          </Card>

          <Card className="p-6 bg-gradient-card border-border/50">
            <div className="flex items-center gap-4">
              <div className="w-12 h-12 rounded-xl bg-gradient-to-br from-accent to-primary flex items-center justify-center">
                <Clock className="w-6 h-6 text-white" />
              </div>
              <div>
                <p className="text-2xl font-bold">{loading ? "..." : stats.active}</p>
                <p className="text-sm text-muted-foreground">Активные</p>
              </div>
            </div>
          </Card>

          <Card className="p-6 bg-gradient-card border-border/50">
            <div className="flex items-center gap-4">
              <div className="w-12 h-12 rounded-xl bg-gradient-to-br from-primary to-accent flex items-center justify-center">
                <AlertCircle className="w-6 h-6 text-white" />
              </div>
              <div>
                <p className="text-2xl font-bold">{loading ? "..." : stats.pendingReview}</p>
                <p className="text-sm text-muted-foreground">Требуют проверки</p>
              </div>
            </div>
          </Card>

          <Card className="p-6 bg-gradient-card border-border/50">
            <div className="flex items-center gap-4">
              <div className="w-12 h-12 rounded-xl bg-gradient-to-br from-accent to-primary flex items-center justify-center">
                <CheckCircle className="w-6 h-6 text-white" />
              </div>
              <div>
                <p className="text-2xl font-bold">{loading ? "..." : stats.completed}</p>
                <p className="text-sm text-muted-foreground">Проверено</p>
              </div>
            </div>
          </Card>
        </div>

        {/* Exams List */}
        <div>
          <h2 className="text-2xl font-bold mb-6">Контрольные работы</h2>
          {loading ? (
            <div className="text-center py-12">
              <p className="text-muted-foreground">Загрузка...</p>
            </div>
          ) : exams.length === 0 ? (
            <Card className="p-12 text-center bg-gradient-card border-border/50">
              <FileText className="w-16 h-16 mx-auto mb-4 text-muted-foreground opacity-50" />
              <h3 className="text-xl font-semibold mb-2">Контрольных работ пока нет</h3>
              <p className="text-muted-foreground mb-6">Создайте первую контрольную работу</p>
              <Link to="/create-exam">
                <Button>
                  <Plus className="w-5 h-5 mr-2" />
                  Создать КР
                </Button>
              </Link>
            </Card>
          ) : (
            <div className="space-y-4">
              {exams.map((exam) => {
                const getStatusLabel = (status: string) => {
                  switch (status) {
                    case 'active': return 'Активна';
                    case 'published': return 'Опубликована';
                    case 'draft': return 'Черновик';
                    case 'completed': return 'Завершена';
                    default: return status;
                  }
                };

                const isActive = exam.status === 'active' || exam.status === 'published';

                return (
                  <Card key={exam.id} className="p-6 hover:shadow-elegant transition-all duration-300 border-border/50 bg-gradient-card">
                    <div className="flex items-center justify-between">
                      <div className="flex-1">
                        <div className="flex items-center gap-3 mb-2">
                          <h3 className="text-xl font-semibold">{exam.title}</h3>
                          <span className={`px-3 py-1 rounded-full text-xs font-medium ${
                            isActive
                              ? "bg-primary/10 text-primary border border-primary/20" 
                              : "bg-muted text-muted-foreground"
                          }`}>
                            {getStatusLabel(exam.status)}
                          </span>
                        </div>
                        <div className="flex items-center gap-6 text-sm text-muted-foreground">
                          <span>Дата: {new Date(exam.start_time).toLocaleDateString("ru-RU", { timeZone: "Europe/Moscow" })}</span>
                          <span>Студентов: {exam.student_count}</span>
                          <span>Задач: {exam.task_count}</span>
                          {exam.pending_count > 0 && (
                            <span className="text-primary font-medium">
                              {exam.pending_count} требуют проверки
                            </span>
                          )}
                        </div>
                      </div>
                      <div className="flex gap-2">
                        <Link to={`/exam/${exam.id}/submissions`}>
                          <Button variant="outline">Проверка</Button>
                        </Link>
                        <Link to={`/exam/${exam.id}/edit`}>
                          <Button variant="ghost">Редактировать</Button>
                        </Link>
                      </div>
                    </div>
                  </Card>
                );
              })}
            </div>
          )}
        </div>
      </div>
    </div>
  );
};

export default TeacherDashboard;
