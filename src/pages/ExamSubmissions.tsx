import { useState, useEffect } from "react";
import { useParams, Link } from "react-router-dom";
import { Navbar } from "@/components/Navbar";
import { Button } from "@/components/ui/button";
import { Card } from "@/components/ui/card";
import { Badge } from "@/components/ui/badge";
import { FileText, Clock, CheckCircle, AlertCircle, Eye } from "lucide-react";
import { examsAPI } from "@/lib/api";
import { toast } from "sonner";

interface Submission {
  id: string;
  student_isu: string;
  student_name: string;
  submitted_at: string;
  status: string;
  ai_score: number | null;
  final_score: number | null;
  max_score: number;
}

const ExamSubmissions = () => {
  const { examId } = useParams<{ examId: string }>();
  const [exam, setExam] = useState<any>(null);
  const [submissions, setSubmissions] = useState<Submission[]>([]);
  const [loading, setLoading] = useState(true);
  const [filter, setFilter] = useState<string | null>(null);

  useEffect(() => {
    const loadData = async () => {
      try {
        const examResponse = await examsAPI.get(examId!);
        setExam(examResponse.data);
        
        const submissionsResponse = await examsAPI.listSubmissions(examId!, filter ? { status: filter } : undefined);
        setSubmissions(submissionsResponse.data);
      } catch (error: any) {
        console.error("Error loading exam data:", error);
        const errorMessage = error.response?.data?.detail || error.message || "Ошибка загрузки данных";
        toast.error(errorMessage);
      } finally {
        setLoading(false);
      }
    };

    if (examId) {
      loadData();
    }
  }, [examId, filter]);

  const getStatusBadge = (status: string) => {
    switch (status) {
      case "preliminary":
        return <Badge variant="outline" className="bg-yellow-50 text-yellow-700 border-yellow-200">
          <Clock className="w-3 h-3 mr-1" />
          Требует проверки
        </Badge>;
      case "processing":
        return <Badge variant="outline" className="bg-blue-50 text-blue-700 border-blue-200">
          <AlertCircle className="w-3 h-3 mr-1" />
          Проверяется ИИ
        </Badge>;
      case "approved":
        return <Badge variant="outline" className="bg-green-50 text-green-700 border-green-200">
          <CheckCircle className="w-3 h-3 mr-1" />
          Одобрено
        </Badge>;
      case "flagged":
        return <Badge variant="outline" className="bg-red-50 text-red-700 border-red-200">
          <AlertCircle className="w-3 h-3 mr-1" />
          Требует ручной проверки
        </Badge>;
      default:
        return <Badge variant="outline">{status}</Badge>;
    }
  };

  if (loading) {
    return (
      <div className="min-h-screen bg-gradient-subtle">
        <Navbar />
        <div className="container mx-auto px-6 pt-24 pb-12">
          <p className="text-muted-foreground">Загрузка...</p>
        </div>
      </div>
    );
  }

  if (!exam) {
    return (
      <div className="min-h-screen bg-gradient-subtle">
        <Navbar />
        <div className="container mx-auto px-6 pt-24 pb-12">
          <Card className="p-6">
            <p className="text-muted-foreground mb-4">Контрольная работа не найдена</p>
            <Link to="/teacher" className="mt-4 inline-block">
              <Button>Вернуться к списку</Button>
            </Link>
          </Card>
        </div>
      </div>
    );
  }

  const pendingCount = submissions.filter(s => s.status === "preliminary").length;
  const approvedCount = submissions.filter(s => s.status === "approved").length;

  return (
    <div className="min-h-screen bg-gradient-subtle">
      <Navbar />
      
      <div className="container mx-auto px-6 pt-24 pb-12">
        <div className="mb-8">
          <div className="flex items-center gap-2 text-sm text-muted-foreground mb-2">
            <Link to="/teacher" className="hover:text-primary">
              Панель преподавателя
            </Link>
            <span>/</span>
            <span>{exam.title}</span>
          </div>
          <h1 className="text-4xl font-bold mb-2">Проверка работ</h1>
          <p className="text-muted-foreground">{exam.title}</p>
        </div>

        {/* Stats */}
        <div className="grid md:grid-cols-3 gap-6 mb-8">
          <Card className="p-6 bg-gradient-card border-border/50">
            <div className="flex items-center gap-4">
              <div className="w-12 h-12 rounded-xl bg-gradient-to-br from-primary to-accent flex items-center justify-center">
                <FileText className="w-6 h-6 text-white" />
              </div>
              <div>
                <p className="text-2xl font-bold">{submissions.length}</p>
                <p className="text-sm text-muted-foreground">Всего работ</p>
              </div>
            </div>
          </Card>

          <Card className="p-6 bg-gradient-card border-border/50">
            <div className="flex items-center gap-4">
              <div className="w-12 h-12 rounded-xl bg-gradient-to-br from-yellow-400 to-orange-400 flex items-center justify-center">
                <Clock className="w-6 h-6 text-white" />
              </div>
              <div>
                <p className="text-2xl font-bold">{pendingCount}</p>
                <p className="text-sm text-muted-foreground">Требуют проверки</p>
              </div>
            </div>
          </Card>

          <Card className="p-6 bg-gradient-card border-border/50">
            <div className="flex items-center gap-4">
              <div className="w-12 h-12 rounded-xl bg-gradient-to-br from-green-400 to-emerald-400 flex items-center justify-center">
                <CheckCircle className="w-6 h-6 text-white" />
              </div>
              <div>
                <p className="text-2xl font-bold">{approvedCount}</p>
                <p className="text-sm text-muted-foreground">Одобрено</p>
              </div>
            </div>
          </Card>
        </div>

        {/* Submissions List */}
        <Card className="p-6">
          <div className="flex items-center justify-between mb-6">
            <h2 className="text-2xl font-bold">Работы студентов</h2>
            <div className="flex gap-2">
              <Button variant={filter === null ? "default" : "outline"} onClick={() => setFilter(null)}>Все</Button>
              <Button variant={filter === "preliminary" ? "default" : "outline"} onClick={() => setFilter("preliminary")}>Требуют проверки</Button>
              <Button variant={filter === "approved" ? "default" : "outline"} onClick={() => setFilter("approved")}>Одобрено</Button>
            </div>
          </div>
          
          {submissions.length === 0 ? (
            <div className="text-center py-12">
              <FileText className="w-16 h-16 mx-auto mb-4 text-muted-foreground opacity-50" />
              <p className="text-muted-foreground">Работы пока не сданы</p>
            </div>
          ) : (
            <div className="space-y-4">
              {submissions.map((submission) => (
                <Card key={submission.id} className="p-4 hover:shadow-lg transition-all duration-300">
                  <div className="flex items-center justify-between">
                    <div className="flex-1">
                      <div className="flex items-center gap-3 mb-2">
                        <h3 className="text-lg font-semibold">{submission.student_name}</h3>
                        <span className="text-sm text-muted-foreground">ISU: {submission.student_isu}</span>
                        {getStatusBadge(submission.status)}
                      </div>
                      <div className="flex items-center gap-6 text-sm text-muted-foreground">
                        <span>
                          Сдано: {new Date(submission.submitted_at).toLocaleString("ru-RU", { timeZone: "Europe/Moscow" })}
                        </span>
                        {submission.final_score !== null && (
                          <span className="text-primary font-semibold">
                            Балл: {submission.final_score.toFixed(1)}/{submission.max_score}
                          </span>
                        )}
                        {submission.ai_score !== null && submission.final_score === null && (
                          <span className="text-blue-600 font-semibold">
                            AI Балл: {submission.ai_score.toFixed(1)}/{submission.max_score}
                          </span>
                        )}
                      </div>
                    </div>
                    <Link to={`/submission/${submission.id}`}>
                      <Button>
                        <Eye className="w-4 h-4 mr-2" />
                        Проверить
                      </Button>
                    </Link>
                  </div>
                </Card>
              ))}
            </div>
          )}
        </Card>
      </div>
    </div>
  );
};

export default ExamSubmissions;

