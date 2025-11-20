import React, { useState, useEffect } from "react";
import { useParams, useNavigate } from "react-router-dom";
import { Navbar } from "@/components/Navbar";
import { Button } from "@/components/ui/button";
import { Card } from "@/components/ui/card";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Textarea } from "@/components/ui/textarea";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";
import { CheckCircle, XCircle, Eye, Edit3, AlertTriangle, RotateCcw, RotateCw, ZoomIn, ZoomOut } from "lucide-react";
import { submissionsAPI } from "@/lib/api";
import { toast } from "sonner";
import ImageLightbox from "@/components/ImageLightbox";
import AiAnalysis from "@/components/AiAnalysis";
import 'katex/dist/katex.min.css';
import { InlineMath, BlockMath } from 'react-katex';

const SubmissionReview = () => {
  const { submissionId } = useParams<{ submissionId: string }>();
  const navigate = useNavigate();

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
  
  const [submission, setSubmission] = useState<any>(null);
  const [loading, setLoading] = useState(true);
  const [editing, setEditing] = useState(false);
  const [imageUrls, setImageUrls] = useState<Record<string, string>>({});
  const [imageAngles, setImageAngles] = useState<Record<string, number>>({});
  const [imageScales, setImageScales] = useState<Record<string, number>>({});
  const [lightboxOpen, setLightboxOpen] = useState(false);
  const [lightboxIndex, setLightboxIndex] = useState(0);
  
  const [overrideScore, setOverrideScore] = useState<number>(0);
  const [teacherComments, setTeacherComments] = useState("");

  useEffect(() => {
    const loadSubmission = async () => {
      try {
        const response = await submissionsAPI.get(submissionId!);
        const submissionData = response.data;
        setSubmission(submissionData);
        setOverrideScore(submissionData.final_score || submissionData.ai_score || 0);
        setTeacherComments(submissionData.teacher_comments || "");
        
        // Load presigned URLs for all images
        if (submissionData.images && submissionData.images.length > 0) {
          const urls: Record<string, string> = {};
          for (const image of submissionData.images) {
            try {
              const urlResponse = await submissionsAPI.getImageViewUrl(image.id);
              if (urlResponse.data.view_url) {
                urls[image.id] = urlResponse.data.view_url;
              } else if (urlResponse.data.file_path) {
                // Local storage fallback
                urls[image.id] = `/api/uploads/${urlResponse.data.file_path}`;
              }
            } catch (error) {
              console.error(`Failed to load URL for image ${image.id}:`, error);
            }
          }
          setImageUrls(urls);
        }
      } catch (error: any) {
        toast.error(error.response?.data?.detail || "Ошибка при загрузке работы");
        navigate(-1);
      } finally {
        setLoading(false);
      }
    };

    if (submissionId) {
      loadSubmission();
    }
  }, [submissionId, navigate]);

  const rotateImage = (id: string, delta: number) => {
    setImageAngles((prev) => ({ ...prev, [id]: ((prev[id] || 0) + delta) % 360 }));
  };

  const zoomImage = (id: string, delta: number) => {
    setImageScales((prev) => {
      const next = Math.min(3, Math.max(0.5, (prev[id] || 1) + delta));
      return { ...prev, [id]: next };
    });
  };

  const handleApprove = async () => {
    try {
      await submissionsAPI.approve(submissionId!, { teacher_comments: teacherComments });
      toast.success("Работа утверждена");
      navigate(-1);
    } catch (error: any) {
      toast.error(error.response?.data?.detail || "Ошибка при утверждении");
    }
  };

  const handleOverride = async () => {
    try {
      await submissionsAPI.overrideScore(submissionId!, {
        final_score: overrideScore,
        teacher_comments: teacherComments,
      });
      toast.success("Оценка изменена");
      setEditing(false);
      // Reload submission
      const response = await submissionsAPI.get(submissionId!);
      setSubmission(response.data);
    } catch (error: any) {
      toast.error(error.response?.data?.detail || "Ошибка при изменении оценки");
    }
  };

  if (loading) {
    return (
      <div className="min-h-screen bg-gradient-subtle">
        <Navbar />
        <div className="container mx-auto px-6 pt-24 pb-12">
          <p>Загрузка работы...</p>
        </div>
      </div>
    );
  }

  if (!submission) {
    return null;
  }

  const scorePercent = submission.max_score > 0
    ? ((submission.final_score || submission.ai_score || 0) / submission.max_score) * 100
    : 0;

  return (
    <div className="min-h-screen bg-gradient-subtle">
      <Navbar />

      <div className="container mx-auto px-6 pt-24 pb-12">
          <div className="mb-8 flex items-center justify-between">
          <div>
            <h1 className="text-4xl font-bold mb-2">Проверка работы</h1>
            <p className="text-muted-foreground">
              Студент: {submission.student_name || submission.student_id} (ISU: {submission.student_isu || '—'}) | Сдано:{" "}
              {new Date(submission.submitted_at).toLocaleString("ru-RU")}
            </p>
          </div>
          <div className="text-right">
            <p className="text-sm text-muted-foreground">Статус</p>
            <p className="text-lg font-semibold">{submission.status}</p>
          </div>
        </div>

        <div className="grid lg:grid-cols-3 gap-6">
          {/* Main Content */}
          <div className="lg:col-span-2 space-y-6">
            {/* Images */}
            <Card className="p-6">
              <h2 className="text-2xl font-bold mb-4">Загруженные изображения</h2>
              {submission.images && submission.images.length > 0 ? (
                <div className="grid md:grid-cols-2 gap-4">
                  {submission.images.map((image: any, index: number) => (
                    <div key={image.id} className="border rounded-lg overflow-hidden">
                      {imageUrls[image.id] ? (
                        <div className="relative">
                          <img
                            src={imageUrls[image.id]}
                            alt={`Page ${index + 1}`}
                            className="w-full h-auto cursor-zoom-in"
                            style={{ transform: `rotate(${imageAngles[image.id] || 0}deg) scale(${imageScales[image.id] || 1})`, transformOrigin: 'center center' }}
                            onError={(e) => {
                              (e.target as HTMLImageElement).src = "/placeholder.svg";
                            }}
                            onClick={() => {
                              // open lightbox at this image index
                              setLightboxIndex(index);
                              setLightboxOpen(true);
                            }}
                          />
                          <div className="absolute top-2 right-2 flex gap-1">
                            <Button size="sm" variant="outline" onClick={() => rotateImage(image.id, -90)}><RotateCcw className="w-4 h-4" /></Button>
                            <Button size="sm" variant="outline" onClick={() => rotateImage(image.id, 90)}><RotateCw className="w-4 h-4" /></Button>
                            <Button size="sm" variant="outline" onClick={() => zoomImage(image.id, -0.25)}><ZoomOut className="w-4 h-4" /></Button>
                            <Button size="sm" variant="outline" onClick={() => zoomImage(image.id, 0.25)}><ZoomIn className="w-4 h-4" /></Button>
                          </div>
                        </div>
                      ) : (
                        <div className="w-full h-64 bg-secondary flex items-center justify-center">
                          <p className="text-muted-foreground">Загрузка изображения...</p>
                        </div>
                      )}
                      <div className="p-2 bg-secondary text-xs">
                        <p>Страница {index + 1}</p>
                        {image.ocr_text && (
                          <p className="mt-1 text-[10px] line-clamp-2 text-muted-foreground">OCR: {image.ocr_text.slice(0, 120)}...</p>
                        )}
                        {image.quality_score && (
                          <p>Качество: {(image.quality_score * 100).toFixed(0)}%</p>
                        )}
                      </div>
                    </div>
                  ))}
                </div>
              ) : (
                <p className="text-muted-foreground">Изображения не загружены</p>
              )}
            </Card>

            {lightboxOpen && (
              <ImageLightbox
                images={submission.images.map((img: any) => imageUrls[img.id]).filter(Boolean)}
                startIndex={lightboxIndex}
                onClose={() => setLightboxOpen(false)}
              />
            )}

            {/* AI Analysis */}
            <Card className="p-6">
              <h2 className="text-2xl font-bold mb-4">Анализ AI</h2>
              
              <Tabs defaultValue="overview">
                <TabsList>
                  <TabsTrigger value="overview">Общее</TabsTrigger>
                  <TabsTrigger value="criteria">По критериям</TabsTrigger>
                  <TabsTrigger value="details">Детали</TabsTrigger>
                  {submission.ai_analysis?.full_transcription_md && (
                    <TabsTrigger value="transcription">Расшифровка</TabsTrigger>
                  )}
                </TabsList>

                <TabsContent value="overview" className="space-y-4">
                  <div>
                    <Label>AI Score:</Label>
                    <p className="text-3xl font-bold">
                      {submission.ai_score || 0} / {submission.max_score}
                    </p>
                  </div>
                  {submission.ai_comments && (
                    <div>
                      <Label>Комментарии AI:</Label>
                      <div className="bg-secondary/50 p-4 rounded mt-2">
                        <div className="whitespace-pre-wrap">{renderLatex(submission.ai_comments)}</div>
                      </div>
                    </div>
                  )}
                </TabsContent>

                <TabsContent value="criteria">
                  {submission.scores && submission.scores.length > 0 ? (
                    <div className="space-y-4">
                      {submission.scores.map((score: any, index: number) => (
                        <div key={index} className="border-b pb-4 last:border-0">
                          <div className="flex items-center justify-between mb-2">
                            <h4 className="font-semibold">{score.criterion_name}</h4>
                            <span className="font-bold">
                              {score.ai_score || 0} / {score.max_score}
                            </span>
                          </div>
                          {score.ai_comment && (
                            <div className="text-sm text-muted-foreground">
                              {renderLatex(score.ai_comment)}
                            </div>
                          )}
                        </div>
                      ))}
                    </div>
                  ) : (
                    <p className="text-muted-foreground">Нет детализации по критериям</p>
                  )}
                </TabsContent>

                <TabsContent value="details">
                  {submission.ai_analysis ? (
                    <AiAnalysis data={submission.ai_analysis} />
                  ) : (
                    <p className="text-muted-foreground">Детальный анализ недоступен</p>
                  )}
                </TabsContent>

                {submission.ai_analysis?.full_transcription_md && (
                  <TabsContent value="transcription">
                    <div className="prose max-w-none">
                      <div className="whitespace-pre-wrap">{renderLatex(submission.ai_analysis.full_transcription_md)}</div>
                    </div>
                  </TabsContent>
                )}
              </Tabs>
            </Card>

            {/* Flags */}
            {submission.is_flagged && submission.flag_reasons.length > 0 && (
              <Card className="p-6 border-yellow-500 bg-yellow-50 dark:bg-yellow-950">
                <h3 className="text-xl font-bold mb-4 flex items-center gap-2">
                  <AlertTriangle className="w-5 h-5 text-yellow-600" />
                  Системные отметки
                </h3>
                <ul className="list-disc list-inside space-y-1">
                  {submission.flag_reasons.map((reason: string, i: number) => (
                    <li key={i}>{reason}</li>
                  ))}
                </ul>
              </Card>
            )}
          </div>

          {/* Sidebar - Actions */}
          <div className="space-y-6">
            {/* Score Card */}
            <Card className="p-6 bg-gradient-card">
              <h3 className="font-semibold mb-4">Итоговая оценка</h3>
              
              {editing ? (
                <div className="space-y-4">
                  <div>
                    <Label htmlFor="override_score">Балл</Label>
                    <Input
                      id="override_score"
                      type="number"
                      min={0}
                      max={submission.max_score}
                      step={0.5}
                      value={overrideScore}
                      onChange={(e) => setOverrideScore(parseFloat(e.target.value))}
                    />
                    <p className="text-xs text-muted-foreground mt-1">
                      Максимум: {submission.max_score}
                    </p>
                  </div>
                  
                  <div>
                    <Label htmlFor="teacher_comments">Комментарии</Label>
                    <Textarea
                      id="teacher_comments"
                      value={teacherComments}
                      onChange={(e) => setTeacherComments(e.target.value)}
                      placeholder="Оставьте комментарий для студента..."
                      rows={6}
                    />
                  </div>

                  <div className="flex gap-2">
                    <Button onClick={handleOverride} className="flex-1">
                      <CheckCircle className="w-4 h-4 mr-2" />
                      Сохранить
                    </Button>
                    <Button
                      variant="outline"
                      onClick={() => setEditing(false)}
                      className="flex-1"
                    >
                      Отмена
                    </Button>
                  </div>
                </div>
              ) : (
                <div className="space-y-4">
                  <div>
                    <p className="text-4xl font-bold">
                      {submission.final_score || submission.ai_score || 0}
                    </p>
                    <p className="text-sm text-muted-foreground">
                      из {submission.max_score} ({scorePercent.toFixed(1)}%)
                    </p>
                  </div>

                  {submission.teacher_comments && (
                    <div className="bg-secondary/50 p-3 rounded text-sm">
                      <p className="font-semibold mb-1">Ваш комментарий:</p>
                      <p>{renderLatex(submission.teacher_comments)}</p>
                    </div>
                  )}

                  <div className="flex flex-col gap-2">
                    <Button
                      onClick={() => setEditing(true)}
                      variant="outline"
                      className="w-full"
                    >
                      <Edit3 className="w-4 h-4 mr-2" />
                      Изменить оценку
                    </Button>
                    
                    {submission.status !== "approved" && (
                      <Button onClick={handleApprove} className="w-full">
                        <CheckCircle className="w-4 h-4 mr-2" />
                        Утвердить
                      </Button>
                    )}
                  </div>
                </div>
              )}
            </Card>

            {/* Quick Actions */}
            <Card className="p-6">
              <h3 className="font-semibold mb-4">Действия</h3>
              <div className="space-y-2">
                <Button variant="outline" className="w-full" onClick={() => navigate(-1)}>
                  Назад к списку
                </Button>
              </div>
            </Card>
          </div>
        </div>
      </div>
    </div>
  );
};

export default SubmissionReview;

