import React, { useState, useEffect } from "react";
import { useParams, Link, useNavigate } from "react-router-dom";
import { Navbar } from "@/components/Navbar";
import { Button } from "@/components/ui/button";
import { Card } from "@/components/ui/card";
import { Progress } from "@/components/ui/progress";
import { CheckCircle, XCircle, Clock, FileText, RefreshCw, Award } from "lucide-react";
import { submissionsAPI } from "@/lib/api";
import { toast } from "sonner";
import AiAnalysis from "@/components/AiAnalysis";
import 'katex/dist/katex.min.css';
import { InlineMath, BlockMath } from 'react-katex';

const ExamResult = () => {
  const { sessionId } = useParams<{ sessionId: string }>();
  const navigate = useNavigate();
  const [submission, setSubmission] = useState<any>(null);
  const [loading, setLoading] = useState(true);
  const [retaking, setRetaking] = useState(false);

  const renderLatex = (text: string) => {
    if (!text) return text;
    
    try {
      // Split by newlines first
      const lines = text.split('\n');
      
      return lines.map((line, lineIndex) => {
        // Process each line for LaTeX
        const parts = line.split(/(\$\$[\s\S]+?\$\$|\$[\s\S]+?\$)/);
        
        const lineContent = parts.map((part, partIndex) => {
          if (part.startsWith("$$") && part.endsWith("$$")) {
            const mathContent = part.slice(2, -2).trim();
            return <BlockMath key={partIndex}>{mathContent}</BlockMath>;
          } else if (part.startsWith("$") && part.endsWith("$")) {
            const mathContent = part.slice(1, -1).trim();
            return <InlineMath key={partIndex}>{mathContent}</InlineMath>;
          }
          return <span key={partIndex}>{part}</span>;
        });
        
        return (
          <span key={lineIndex}>
            {lineContent}
            {lineIndex < lines.length - 1 && <br />}
          </span>
        );
      });
    } catch (error) {
      console.error('Error rendering LaTeX:', error);
      return <span>{text}</span>;
    }
  };

  useEffect(() => {
    const loadResult = async () => {
      try {
        const response = await submissionsAPI.getResult(sessionId!);
        setSubmission(response.data);
      } catch (error: any) {
        toast.error(error.response?.data?.detail || "Ошибка при загрузке результата");
      } finally {
        setLoading(false);
      }
    };

    if (sessionId) {
      loadResult();
    }
  }, [sessionId]);

  if (loading) {
    return (
      <div className="min-h-screen bg-gradient-subtle">
        <Navbar />
        <div className="container mx-auto px-6 pt-24 pb-12">
          <p>Загрузка результатов...</p>
        </div>
      </div>
    );
  }

  if (!submission) {
    return (
      <div className="min-h-screen bg-gradient-subtle">
        <Navbar />
        <div className="container mx-auto px-6 pt-24 pb-12">
          <Card className="p-8 text-center">
            <XCircle className="w-16 h-16 mx-auto mb-4 text-red-500" />
            <h2 className="text-2xl font-bold mb-2">Результат не найден</h2>
            <p className="text-muted-foreground mb-6">
              Возможно, работа еще не была сдана
            </p>
            <Link to="/student">
              <Button>Вернуться к списку</Button>
            </Link>
          </Card>
        </div>
      </div>
    );
  }

  const scorePercent = submission.max_score > 0
    ? ((submission.final_score || submission.ai_score || 0) / submission.max_score) * 100
    : 0;

  // Check if student can retake the exam
  const canRetake = submission.exam && submission.session && 
    submission.status === "approved" &&
    submission.session.total_attempts < submission.exam.max_attempts;

  const handleRetake = async () => {
    if (!submission.exam) return;
    
    setRetaking(true);
    try {
      // Navigate to the exam page to start a new attempt
      navigate(`/exam/${submission.exam.id}`);
      toast.success("Начинаем новую попытку...");
    } catch (error: any) {
      toast.error(error.response?.data?.detail || "Ошибка при начале новой попытки");
      setRetaking(false);
    }
  };

  const getStatusColor = (status: string) => {
    switch (status) {
      case "approved":
        return "text-green-600";
      case "processing":
        return "text-yellow-600";
      case "preliminary":
        return "text-blue-600";
      case "flagged":
        return "text-red-600";
      default:
        return "text-gray-600";
    }
  };

  const getStatusText = (status: string) => {
    switch (status) {
      case "approved":
        return "Проверено";
      case "processing":
        return "В обработке";
      case "preliminary":
        return "Предварительная проверка";
      case "flagged":
        return "Требует внимания";
      default:
        return status;
    }
  };

  return (
    <div className="min-h-screen bg-gradient-subtle">
      <Navbar />

      <div className="container mx-auto px-6 pt-24 pb-12">
        <div className="mb-8">
          <div className="flex items-center justify-between">
            <div>
              <h1 className="text-4xl font-bold mb-2">Результаты работы</h1>
              <p className="text-muted-foreground">
                Сдано: {new Date(submission.submitted_at).toLocaleString("ru-RU")}
              </p>
              {submission.exam && submission.session && (
                <p className="text-sm text-muted-foreground mt-1">
                  Попытка {submission.session.attempt_number} из {submission.exam.max_attempts}
                </p>
              )}
            </div>
            {canRetake && (
              <Button 
                size="lg" 
                onClick={handleRetake}
                disabled={retaking}
                className="gap-2"
              >
                <RefreshCw className={`w-5 h-5 ${retaking ? 'animate-spin' : ''}`} />
                Пройти повторно
              </Button>
            )}
          </div>
        </div>

        {/* Score Card */}
        <Card className="p-8 mb-8 bg-gradient-card">
          <div className="flex items-center justify-between mb-6">
            <div>
              <p className="text-sm text-muted-foreground mb-1">Ваш балл</p>
              <h2 className="text-5xl font-bold">
                {submission.final_score || submission.ai_score || 0}{" "}
                <span className="text-2xl text-muted-foreground">
                  / {submission.max_score}
                </span>
              </h2>
            </div>
            <div className="text-right">
              <p className={`text-lg font-semibold ${getStatusColor(submission.status)}`}>
                {getStatusText(submission.status)}
              </p>
              <p className="text-3xl font-bold mt-2">{scorePercent.toFixed(1)}%</p>
            </div>
          </div>
          <Progress value={scorePercent} className="h-3" />
        </Card>

        {/* Processing Status */}
        {submission.status === "processing" && (
          <Card className="p-6 mb-8 border-yellow-500">
            <div className="flex items-center gap-4">
              <Clock className="w-8 h-8 text-yellow-600 animate-pulse" />
              <div>
                <h3 className="font-semibold text-lg">Работа обрабатывается</h3>
                <p className="text-muted-foreground">
                  Пожалуйста, подождите. AI анализирует ваше решение. Это может занять несколько минут.
                </p>
              </div>
            </div>
          </Card>
        )}

        {/* AI Comments */}
        {submission.ai_comments && (
          <Card className="p-6 mb-8">
            <h3 className="text-xl font-bold mb-4 flex items-center gap-2">
              <FileText className="w-5 h-5" />
              Комментарии AI
            </h3>
            <div className="prose max-w-none">
              <p className="whitespace-pre-wrap">{renderLatex(submission.ai_comments)}</p>
            </div>
          </Card>
        )}

        {/* Teacher Comments */}
        {submission.teacher_comments && (
          <Card className="p-6 mb-8 border-primary">
            <h3 className="text-xl font-bold mb-4 flex items-center gap-2">
              <FileText className="w-5 h-5 text-primary" />
              Комментарии преподавателя
            </h3>
            <div className="prose max-w-none">
              <p className="whitespace-pre-wrap">{renderLatex(submission.teacher_comments)}</p>
            </div>
          </Card>
        )}

        {/* Detailed Scores by Task/Question */}
        {submission.scores && submission.scores.length > 0 && (
          <Card className="p-6 mb-8 border-primary/20">
            <div className="flex items-center gap-2 mb-6">
              <Award className="w-6 h-6 text-primary" />
              <h3 className="text-2xl font-bold">Разбалловка по заданиям</h3>
            </div>
            <div className="space-y-6">
              {submission.scores.map((score: any, index: number) => {
                const taskScore = score.final_score !== null ? score.final_score : score.ai_score || 0;
                const taskPercent = score.max_score > 0 ? (taskScore / score.max_score) * 100 : 0;
                
                return (
                  <div key={index} className="border rounded-lg p-4 bg-gradient-to-r from-background to-muted/20">
                    <div className="flex items-center justify-between mb-3">
                      <div className="flex-1">
                        <h4 className="font-bold text-lg">{score.criterion_name}</h4>
                        {score.criterion_description && (
                          <p className="text-sm text-muted-foreground mt-1">
                            {score.criterion_description}
                          </p>
                        )}
                      </div>
                      <div className="text-right ml-4">
                        <div className="text-2xl font-bold text-primary">
                          {taskScore.toFixed(1)} / {score.max_score}
                        </div>
                        <div className="text-sm text-muted-foreground">
                          {taskPercent.toFixed(0)}%
                        </div>
                      </div>
                    </div>
                    
                    <Progress value={taskPercent} className="h-2 mb-3" />
                    
                    {score.teacher_comment && (
                      <div className="mt-3 p-3 bg-primary/10 border border-primary/20 rounded">
                        <p className="text-sm font-semibold text-primary mb-1">
                          Комментарий преподавателя:
                        </p>
                        <p className="text-sm whitespace-pre-wrap">{score.teacher_comment}</p>
                      </div>
                    )}
                    
                    {!score.teacher_comment && score.ai_comment && (
                      <div className="mt-3 p-3 bg-secondary/50 rounded">
                        <p className="text-sm font-semibold mb-1">
                          Комментарий AI:
                        </p>
                        <div className="text-sm whitespace-pre-wrap">{renderLatex(score.ai_comment)}</div>
                      </div>
                    )}
                  </div>
                );
              })}
            </div>
          </Card>
        )}

        {/* AI Analysis */}
        {submission.ai_analysis && (
          <Card className="p-6 mb-8">
            <h3 className="text-xl font-bold mb-4">Детальный анализ</h3>
            <AiAnalysis data={submission.ai_analysis} />
          </Card>
        )}

        {/* Recommendations */}
        {submission.ai_analysis?.recommendations &&
          submission.ai_analysis.recommendations.length > 0 && (
            <Card className="p-6 mb-8 bg-blue-50 dark:bg-blue-950">
              <h3 className="text-xl font-bold mb-4">Рекомендации</h3>
              <ul className="list-disc list-inside space-y-2">
                {submission.ai_analysis.recommendations.map((rec: string, i: number) => (
                  <li key={i}>{renderLatex(rec)}</li>
                ))}
              </ul>
            </Card>
          )}

        {/* Flags */}
        {submission.is_flagged && submission.flag_reasons.length > 0 && (
          <Card className="p-6 mb-8 border-yellow-500 bg-yellow-50 dark:bg-yellow-950">
            <h3 className="text-xl font-bold mb-4 flex items-center gap-2">
              <XCircle className="w-5 h-5 text-yellow-600" />
              Отметки системы
            </h3>
            <ul className="list-disc list-inside space-y-1">
              {submission.flag_reasons.map((reason: string, i: number) => (
                <li key={i}>{reason}</li>
              ))}
            </ul>
          </Card>
        )}

        {/* Retake Notice */}
        {canRetake && (
          <Card className="p-6 mb-8 bg-gradient-to-r from-primary/10 to-accent/10 border-primary/30">
            <div className="flex items-center gap-4">
              <div className="w-12 h-12 rounded-full bg-primary/20 flex items-center justify-center">
                <RefreshCw className="w-6 h-6 text-primary" />
              </div>
              <div className="flex-1">
                <h3 className="font-bold text-lg mb-1">Доступна повторная попытка</h3>
                <p className="text-sm text-muted-foreground">
                  У вас осталось {submission.exam.max_attempts - submission.session.total_attempts} попыток из {submission.exam.max_attempts}. 
                  Вы можете пройти контрольную работу еще раз для улучшения результата.
                </p>
              </div>
              <Button 
                size="lg" 
                onClick={handleRetake}
                disabled={retaking}
                className="gap-2"
              >
                <RefreshCw className={`w-5 h-5 ${retaking ? 'animate-spin' : ''}`} />
                Начать новую попытку
              </Button>
            </div>
          </Card>
        )}

        <div className="text-center">
          <Link to="/student">
            <Button size="lg" variant="outline">Вернуться к списку экзаменов</Button>
          </Link>
        </div>
      </div>
    </div>
  );
};

export default ExamResult;

