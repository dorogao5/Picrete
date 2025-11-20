import { useState, useEffect } from "react";
import { Link } from "react-router-dom";
import { Button } from "@/components/ui/button";
import { Card } from "@/components/ui/card";
import { Navbar } from "@/components/Navbar";
import { Calendar, Clock, CheckCircle, FileText } from "lucide-react";
import { examsAPI, submissionsAPI } from "@/lib/api";
import { toast } from "sonner";

interface ExamSummary {
  id: string;
  title: string;
  start_time: string;
  end_time: string;
  duration_minutes: number;
  status: string;
}

interface StudentSubmission {
  id: string;
  session_id: string;
  exam_id: string;
  exam_title: string;
  submitted_at: string;
  status: string;
  ai_score: number | null;
  final_score: number | null;
  max_score: number;
  teacher_comments: string | null;
}

const StudentDashboard = () => {
  const [exams, setExams] = useState<ExamSummary[]>([]);
  const [submissions, setSubmissions] = useState<StudentSubmission[]>([]);
  const [loading, setLoading] = useState(true);

  useEffect(() => {
    const fetchData = async () => {
      // Проверяем наличие токена перед запросами
      const token = localStorage.getItem('access_token');
      if (!token) {
        // Если токена нет, не делаем запросы - interceptor обработает редирект
        setLoading(false);
        return;
      }
      
      try {
        const [examsResponse, submissionsResponse] = await Promise.all([
          examsAPI.list(),
          submissionsAPI.mySubmissions(),
        ]);
        setExams(examsResponse.data);
        setSubmissions(submissionsResponse.data);
      } catch (error: any) {
        console.error("Error fetching data:", error);
        // Не показываем ошибку для 401 - interceptor сам обработает редирект
        if (error.response?.status === 401) {
          setLoading(false);
          return;
        }
        // Для других ошибок показываем уведомление
        toast.error("Ошибка загрузки данных");
      } finally {
        setLoading(false);
      }
    };

    fetchData();
  }, []);

  // Separate exams into upcoming and completed based on submissions
  const submittedExamIds = new Set(submissions.map(s => s.exam_id));
  const now = new Date();
  
  // Show all published/active exams that student hasn't submitted yet
  // Backend returns statuses in lowercase: 'published', 'active'
  const upcomingExams = exams.filter(exam => {
    return !submittedExamIds.has(exam.id) && 
           (exam.status === 'published' || exam.status === 'active');
  });

  const completedSubmissions = submissions.filter(s => s.status !== 'pending');

  const formatDateTime = (dateString: string) => {
    // Backend stores time in UTC, convert to GMT+3 (Moscow time)
    const date = new Date(dateString);
    return {
      date: date.toLocaleDateString("ru-RU", { timeZone: "Europe/Moscow" }),
      time: date.toLocaleTimeString("ru-RU", { hour: "2-digit", minute: "2-digit", timeZone: "Europe/Moscow" }),
    };
  };

  // Calculate average score
  const averageScore = completedSubmissions.length > 0
    ? completedSubmissions.reduce((sum, s) => {
        const score = s.final_score !== null ? s.final_score : (s.ai_score || 0);
        const percentage = s.max_score > 0 ? (score / s.max_score) * 100 : 0;
        return sum + percentage;
      }, 0) / completedSubmissions.length
    : 0;

  return (
    <div className="min-h-screen bg-gradient-subtle">
      <Navbar />
      
      <div className="container mx-auto px-6 pt-24 pb-12">
        <div className="mb-8">
          <h1 className="text-4xl font-bold mb-2">Панель студента</h1>
          <p className="text-muted-foreground">Расписание контрольных работ и результаты</p>
        </div>

        {/* Stats Overview */}
        <div className="grid md:grid-cols-3 gap-6 mb-8">
          <Card className="p-6 bg-gradient-card border-border/50">
            <div className="flex items-center gap-4">
              <div className="w-12 h-12 rounded-xl bg-gradient-to-br from-primary to-accent flex items-center justify-center">
                <Calendar className="w-6 h-6 text-white" />
              </div>
              <div>
                <p className="text-2xl font-bold">{loading ? "..." : upcomingExams.length}</p>
                <p className="text-sm text-muted-foreground">Предстоящие КР</p>
              </div>
            </div>
          </Card>

          <Card className="p-6 bg-gradient-card border-border/50">
            <div className="flex items-center gap-4">
              <div className="w-12 h-12 rounded-xl bg-gradient-to-br from-accent to-primary flex items-center justify-center">
                <CheckCircle className="w-6 h-6 text-white" />
              </div>
              <div>
                <p className="text-2xl font-bold">{loading ? "..." : completedSubmissions.length}</p>
                <p className="text-sm text-muted-foreground">Выполнено</p>
              </div>
            </div>
          </Card>

          <Card className="p-6 bg-gradient-card border-border/50">
            <div className="flex items-center gap-4">
              <div className="w-12 h-12 rounded-xl bg-gradient-to-br from-primary to-accent flex items-center justify-center">
                <FileText className="w-6 h-6 text-white" />
              </div>
              <div>
                <p className="text-2xl font-bold">
                  {loading ? "..." : averageScore.toFixed(1)}
                </p>
                <p className="text-sm text-muted-foreground">Средний балл %</p>
              </div>
            </div>
          </Card>
        </div>

        {/* Upcoming Exams */}
        <div className="mb-8">
          <h2 className="text-2xl font-bold mb-6">Предстоящие контрольные</h2>
          {loading ? (
            <div className="text-center py-12">
              <p className="text-muted-foreground">Загрузка...</p>
            </div>
          ) : upcomingExams.length === 0 ? (
            <Card className="p-8 text-center bg-gradient-card border-border/50">
              <Calendar className="w-12 h-12 mx-auto mb-3 text-muted-foreground opacity-50" />
              <p className="text-muted-foreground">Нет предстоящих контрольных работ</p>
            </Card>
          ) : (
            <div className="space-y-4">
              {upcomingExams.map((exam) => {
                const { date, time } = formatDateTime(exam.start_time);
                // Backend stores time in UTC, Date constructor parses it correctly
                const startTime = new Date(exam.start_time);
                const endTime = new Date(exam.end_time);
                const isAvailable = now >= startTime && now <= endTime;
                const isExpired = now > endTime;
                
                return (
                  <Card key={exam.id} className="p-6 hover:shadow-elegant transition-all duration-300 border-border/50 bg-gradient-card">
                    <div className="flex items-center justify-between">
                      <div className="flex-1">
                        <h3 className="text-xl font-semibold mb-2">{exam.title}</h3>
                        <div className="flex items-center gap-6 text-sm text-muted-foreground">
                          <div className="flex items-center gap-2">
                            <Calendar className="w-4 h-4" />
                            <span>{date}</span>
                          </div>
                          <div className="flex items-center gap-2">
                            <Clock className="w-4 h-4" />
                            <span>{time} ({exam.duration_minutes} мин)</span>
                          </div>
                          {isExpired && (
                            <span className="text-destructive font-medium">Время истекло</span>
                          )}
                        </div>
                      </div>
                      {isAvailable ? (
                        <Link to={`/exam/${exam.id}`}>
                          <Button className="shadow-soft">
                            Начать
                          </Button>
                        </Link>
                      ) : (
                        <Button disabled variant="outline">
                          {now < startTime ? "Скоро" : "Завершена"}
                        </Button>
                      )}
                    </div>
                  </Card>
                );
              })}
            </div>
          )}
        </div>

        {/* Completed Exams */}
        <div>
          <h2 className="text-2xl font-bold mb-6">Выполненные работы</h2>
          {loading ? (
            <div className="text-center py-12">
              <p className="text-muted-foreground">Загрузка...</p>
            </div>
          ) : completedSubmissions.length === 0 ? (
            <Card className="p-8 text-center bg-gradient-card border-border/50">
              <CheckCircle className="w-12 h-12 mx-auto mb-3 text-muted-foreground opacity-50" />
              <p className="text-muted-foreground">Нет выполненных работ</p>
            </Card>
          ) : (
            <div className="space-y-4">
              {completedSubmissions.map((submission) => {
                const score = submission.final_score !== null ? submission.final_score : submission.ai_score;
                const scorePercentage = submission.max_score > 0 
                  ? ((score || 0) / submission.max_score * 100).toFixed(1)
                  : 0;
                
                const getStatusLabel = (status: string) => {
                  switch (status) {
                    case 'preliminary': return 'На проверке';
                    case 'approved': return 'Проверено';
                    default: return status;
                  }
                };
                
                return (
                  <Card key={submission.id} className="p-6 hover:shadow-elegant transition-all duration-300 border-border/50 bg-gradient-card">
                    <div className="flex items-center justify-between">
                      <div className="flex-1">
                        <div className="flex items-center gap-3 mb-2">
                          <h3 className="text-xl font-semibold">{submission.exam_title}</h3>
                          <span className="px-3 py-1 rounded-full text-xs font-medium bg-muted text-muted-foreground">
                            {getStatusLabel(submission.status)}
                          </span>
                        </div>
                        <div className="flex items-center gap-6 text-sm text-muted-foreground">
                          <span>Дата: {new Date(submission.submitted_at).toLocaleDateString("ru-RU", { timeZone: "Europe/Moscow" })}</span>
                          {score !== null && (
                            <span className="text-primary font-bold text-lg">
                              Балл: {score.toFixed(1)}/{submission.max_score} ({scorePercentage}%)
                            </span>
                          )}
                        </div>
                      </div>
                      <Link to={`/exam/${submission.session_id}/result`}>
                        <Button variant="outline">
                          Посмотреть результат
                        </Button>
                      </Link>
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

export default StudentDashboard;
