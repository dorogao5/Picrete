import { useState, useEffect } from "react";
import { useNavigate, useParams } from "react-router-dom";
import { Navbar } from "@/components/Navbar";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Textarea } from "@/components/ui/textarea";
import { Card } from "@/components/ui/card";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";
import { AlertDialog, AlertDialogAction, AlertDialogCancel, AlertDialogContent, AlertDialogDescription, AlertDialogFooter, AlertDialogHeader, AlertDialogTitle, AlertDialogTrigger } from "@/components/ui/alert-dialog";
import { Plus, Trash2, Save } from "lucide-react";
import { examsAPI } from "@/lib/api";
import { toast } from "sonner";

interface TaskVariant {
  content: string;
  parameters: Record<string, any>;
  reference_solution: string;
  reference_answer: string;
  answer_tolerance: number;
}

interface TaskType {
  title: string;
  description: string;
  order_index: number;
  max_score: number;
  rubric: Record<string, any>;
  difficulty: "easy" | "medium" | "hard";
  taxonomy_tags: string[];
  formulas: string[];
  units: string[];
  validation_rules: Record<string, any>;
  variants: TaskVariant[];
}

const CreateExam = () => {
  const navigate = useNavigate();
  const { examId } = useParams<{ examId?: string }>();
  const isEditMode = !!examId;
  
  const [loading, setLoading] = useState(false);
  const [initialLoading, setInitialLoading] = useState(isEditMode);
  const [showForceDeleteDialog, setShowForceDeleteDialog] = useState(false);
  const [submissionCount, setSubmissionCount] = useState(0);
  
  const [examData, setExamData] = useState({
    title: "",
    description: "",
    start_time: "",
    end_time: "",
    duration_minutes: 90,
    timezone: "Europe/Moscow", // GMT+3
    max_attempts: 1,
    allow_breaks: false,
    break_duration_minutes: 0,
  });

  const [taskTypes, setTaskTypes] = useState<TaskType[]>([]);

  // Load exam data if editing
  useEffect(() => {
    const loadExam = async () => {
      if (!examId) return;
      
      try {
        const response = await examsAPI.get(examId);
        const exam = response.data;
        
        // Convert UTC time back to Moscow time for the datetime-local input
        const startTime = new Date(exam.start_time + 'Z'); // Add Z to indicate UTC
        const endTime = new Date(exam.end_time + 'Z');
        
        // Format for datetime-local input (YYYY-MM-DDTHH:mm)
        const formatForInput = (date: Date) => {
          const year = date.getFullYear();
          const month = String(date.getMonth() + 1).padStart(2, '0');
          const day = String(date.getDate()).padStart(2, '0');
          const hours = String(date.getHours()).padStart(2, '0');
          const minutes = String(date.getMinutes()).padStart(2, '0');
          return `${year}-${month}-${day}T${hours}:${minutes}`;
        };
        
        setExamData({
          title: exam.title,
          description: exam.description || "",
          start_time: formatForInput(startTime),
          end_time: formatForInput(endTime),
          duration_minutes: exam.duration_minutes,
          timezone: exam.timezone,
          max_attempts: exam.max_attempts,
          allow_breaks: exam.allow_breaks,
          break_duration_minutes: exam.break_duration_minutes || 0,
        });
        
        // Load task types
        if (exam.task_types && exam.task_types.length > 0) {
          setTaskTypes(exam.task_types.map((tt: any) => ({
            title: tt.title,
            description: tt.description,
            order_index: tt.order_index,
            max_score: tt.max_score,
            rubric: tt.rubric,
            difficulty: tt.difficulty,
            taxonomy_tags: tt.taxonomy_tags || [],
            formulas: tt.formulas || [],
            units: tt.units || [],
            validation_rules: tt.validation_rules || {},
            variants: tt.variants.map((v: any) => ({
              content: v.content,
              parameters: v.parameters || {},
              reference_solution: v.reference_solution || "",
              reference_answer: v.reference_answer || "",
              answer_tolerance: v.answer_tolerance || 0.01,
            })),
          })));
        }
      } catch (error: any) {
        toast.error("Ошибка загрузки контрольной работы");
        navigate("/teacher");
      } finally {
        setInitialLoading(false);
      }
    };
    
    loadExam();
  }, [examId, navigate]);

  const addTaskType = () => {
    setTaskTypes([
      ...taskTypes,
      {
        title: "",
        description: "",
        order_index: taskTypes.length,
        max_score: 10,
        rubric: {
          criteria: [
            { name: "Корректность метода", weight: 0.3 },
            { name: "Вычисления", weight: 0.3 },
            { name: "Единицы измерения", weight: 0.2 },
            { name: "Оформление ответа", weight: 0.2 },
          ],
        },
        difficulty: "medium",
        taxonomy_tags: [],
        formulas: [],
        units: [],
        validation_rules: {},
        variants: [
          {
            content: "",
            parameters: {},
            reference_solution: "",
            reference_answer: "",
            answer_tolerance: 0.01,
          },
        ],
      },
    ]);
  };

  const removeTaskType = (index: number) => {
    setTaskTypes(taskTypes.filter((_, i) => i !== index));
  };

  const updateTaskType = (index: number, field: keyof TaskType, value: any) => {
    const updated = [...taskTypes];
    updated[index] = { ...updated[index], [field]: value };
    setTaskTypes(updated);
  };

  const addVariant = (taskIndex: number) => {
    const updated = [...taskTypes];
    updated[taskIndex].variants.push({
      content: "",
      parameters: {},
      reference_solution: "",
      reference_answer: "",
      answer_tolerance: 0.01,
    });
    setTaskTypes(updated);
  };

  const removeVariant = (taskIndex: number, variantIndex: number) => {
    const updated = [...taskTypes];
    updated[taskIndex].variants = updated[taskIndex].variants.filter(
      (_, i) => i !== variantIndex
    );
    setTaskTypes(updated);
  };

  const updateVariant = (
    taskIndex: number,
    variantIndex: number,
    field: keyof TaskVariant,
    value: any
  ) => {
    const updated = [...taskTypes];
    updated[taskIndex].variants[variantIndex] = {
      ...updated[taskIndex].variants[variantIndex],
      [field]: value,
    };
    setTaskTypes(updated);
  };

  const handleSubmit = async (publish: boolean = false) => {
    setLoading(true);
    try {
      // Validate
      if (!examData.title || !examData.start_time || !examData.end_time) {
        toast.error("Заполните все обязательные поля");
        return;
      }

      if (taskTypes.length === 0) {
        toast.error("Добавьте хотя бы один тип задачи");
        return;
      }

      // Convert Moscow time (GMT+3) to UTC for storage
      // datetime-local input gives us naive datetime, interpret it as Moscow time (GMT+3)
      // by appending +03:00 timezone offset
      const startTimeUTC = new Date(examData.start_time + ':00+03:00').toISOString();
      const endTimeUTC = new Date(examData.end_time + ':00+03:00').toISOString();

      let resultExamId = examId;

      if (isEditMode) {
        // Update existing exam
        await examsAPI.update(examId!, {
          ...examData,
          start_time: startTimeUTC,
          end_time: endTimeUTC,
          task_types: taskTypes,
        });
        toast.success("Контрольная работа обновлена");
      } else {
        // Create new exam
        const response = await examsAPI.create({
          ...examData,
          start_time: startTimeUTC,
          end_time: endTimeUTC,
          task_types: taskTypes,
        });
        resultExamId = response.data.id;
        toast.success("Контрольная работа создана");
      }

      if (publish && resultExamId) {
        await examsAPI.publish(resultExamId);
        toast.success("Контрольная работа опубликована");
      }

      navigate("/teacher");
    } catch (error: any) {
      toast.error(error.response?.data?.detail || `Ошибка при ${isEditMode ? 'обновлении' : 'создании'} КР`);
    } finally {
      setLoading(false);
    }
  };

  const handleDelete = async (forceDelete: boolean = false) => {
    if (!examId) return;
    
    setLoading(true);
    try {
      await examsAPI.delete(examId, forceDelete);
      toast.success("Контрольная работа удалена");
      navigate("/teacher");
    } catch (error: any) {
      const errorDetail = error.response?.data?.detail;
      
      // Check if error is about existing submissions
      if (errorDetail && errorDetail.includes("existing submission") && !forceDelete) {
        // Extract count from error message: "Cannot delete exam with X existing submission(s)..."
        const match = errorDetail.match(/with (\d+) existing/);
        const count = match ? parseInt(match[1]) : 0;
        setSubmissionCount(count);
        setShowForceDeleteDialog(true);
      } else {
        toast.error(errorDetail || "Ошибка при удалении КР");
      }
    } finally {
      setLoading(false);
    }
  };

  const handleForceDelete = async () => {
    setShowForceDeleteDialog(false);
    await handleDelete(true);
  };

  if (initialLoading) {
    return (
      <div className="min-h-screen bg-gradient-subtle">
        <Navbar />
        <div className="container mx-auto px-6 pt-24 pb-12">
          <div className="text-center py-12">
            <p className="text-muted-foreground">Загрузка...</p>
          </div>
        </div>
      </div>
    );
  }

  return (
    <div className="min-h-screen bg-gradient-subtle">
      <Navbar />

      <div className="container mx-auto px-6 pt-24 pb-12">
        <div className="mb-8 flex items-start justify-between">
          <div>
            <h1 className="text-4xl font-bold mb-2">
              {isEditMode ? "Редактирование контрольной работы" : "Создание контрольной работы"}
            </h1>
            <p className="text-muted-foreground">
              Настройте параметры КР и добавьте задачи
            </p>
          </div>
          {isEditMode && (
            <>
              <AlertDialog>
                <AlertDialogTrigger asChild>
                  <Button variant="destructive" disabled={loading}>
                    <Trash2 className="w-4 h-4 mr-2" />
                    Удалить КР
                  </Button>
                </AlertDialogTrigger>
                <AlertDialogContent>
                  <AlertDialogHeader>
                    <AlertDialogTitle>Вы уверены?</AlertDialogTitle>
                    <AlertDialogDescription>
                      Это действие нельзя отменить. Контрольная работа будет удалена навсегда.
                      {examData.title && ` Будет удалена: "${examData.title}"`}
                    </AlertDialogDescription>
                  </AlertDialogHeader>
                  <AlertDialogFooter>
                    <AlertDialogCancel>Отмена</AlertDialogCancel>
                    <AlertDialogAction onClick={() => handleDelete(false)} className="bg-destructive text-destructive-foreground hover:bg-destructive/90">
                      Удалить
                    </AlertDialogAction>
                  </AlertDialogFooter>
                </AlertDialogContent>
              </AlertDialog>

              {/* Force delete confirmation dialog */}
              <AlertDialog open={showForceDeleteDialog} onOpenChange={setShowForceDeleteDialog}>
                <AlertDialogContent>
                  <AlertDialogHeader>
                    <AlertDialogTitle className="text-destructive">⚠️ Внимание! Существуют работы студентов</AlertDialogTitle>
                    <AlertDialogDescription className="space-y-3">
                      <p className="font-semibold">
                        У этой контрольной работы есть {submissionCount} {submissionCount === 1 ? 'работа студента' : 'работы студентов'}.
                      </p>
                      <p>
                        При удалении будут безвозвратно удалены:
                      </p>
                      <ul className="list-disc list-inside space-y-1 ml-2">
                        <li>Все работы студентов</li>
                        <li>Загруженные файлы и изображения</li>
                        <li>Оценки и комментарии</li>
                        <li>История попыток</li>
                      </ul>
                      <p className="font-semibold text-destructive">
                        Это действие невозможно отменить!
                      </p>
                    </AlertDialogDescription>
                  </AlertDialogHeader>
                  <AlertDialogFooter>
                    <AlertDialogCancel>Отмена</AlertDialogCancel>
                    <AlertDialogAction 
                      onClick={handleForceDelete} 
                      className="bg-destructive text-destructive-foreground hover:bg-destructive/90"
                    >
                      Да, удалить всё
                    </AlertDialogAction>
                  </AlertDialogFooter>
                </AlertDialogContent>
              </AlertDialog>
            </>
          )}
        </div>

        <div className="grid gap-6">
          {/* Basic Info */}
          <Card className="p-6">
            <h2 className="text-2xl font-bold mb-4">Основная информация</h2>
            <div className="grid md:grid-cols-2 gap-4">
              <div className="md:col-span-2">
                <Label htmlFor="title">Название КР *</Label>
                <Input
                  id="title"
                  value={examData.title}
                  onChange={(e) =>
                    setExamData({ ...examData, title: e.target.value })
                  }
                  placeholder="Термодинамика и кислотно-основные реакции"
                />
              </div>
              <div className="md:col-span-2">
                <Label htmlFor="description">Описание</Label>
                <Textarea
                  id="description"
                  value={examData.description}
                  onChange={(e) =>
                    setExamData({ ...examData, description: e.target.value })
                  }
                  placeholder="Краткое описание контрольной работы..."
                  rows={3}
                />
              </div>
              <div>
                <Label htmlFor="start_time">Дата и время начала *</Label>
                <Input
                  id="start_time"
                  type="datetime-local"
                  value={examData.start_time}
                  onChange={(e) =>
                    setExamData({ ...examData, start_time: e.target.value })
                  }
                />
              </div>
              <div>
                <Label htmlFor="end_time">Дата и время окончания *</Label>
                <Input
                  id="end_time"
                  type="datetime-local"
                  value={examData.end_time}
                  onChange={(e) =>
                    setExamData({ ...examData, end_time: e.target.value })
                  }
                />
              </div>
              <div>
                <Label htmlFor="duration">Длительность (минут) *</Label>
                <Input
                  id="duration"
                  type="number"
                  value={examData.duration_minutes}
                  onChange={(e) =>
                    setExamData({
                      ...examData,
                      duration_minutes: parseInt(e.target.value) || 90,
                    })
                  }
                />
              </div>
              <div>
                <Label htmlFor="max_attempts">Максимум попыток</Label>
                <Input
                  id="max_attempts"
                  type="number"
                  value={examData.max_attempts}
                  onChange={(e) =>
                    setExamData({
                      ...examData,
                      max_attempts: parseInt(e.target.value) || 1,
                    })
                  }
                />
              </div>
            </div>
          </Card>

          {/* Task Types */}
          <Card className="p-6">
            <div className="flex items-center justify-between mb-4">
              <h2 className="text-2xl font-bold">Задачи</h2>
              <Button onClick={addTaskType}>
                <Plus className="w-4 h-4 mr-2" />
                Добавить задачу
              </Button>
            </div>

            {taskTypes.length === 0 ? (
              <p className="text-muted-foreground text-center py-8">
                Нажмите "Добавить задачу", чтобы начать
              </p>
            ) : (
              <div className="space-y-6">
                {taskTypes.map((taskType, taskIndex) => (
                  <Card key={taskIndex} className="p-4 border-2">
                    <div className="flex items-start justify-between mb-4">
                      <h3 className="text-lg font-semibold">
                        Задача {taskIndex + 1}
                      </h3>
                      <Button
                        variant="ghost"
                        size="sm"
                        onClick={() => removeTaskType(taskIndex)}
                      >
                        <Trash2 className="w-4 h-4" />
                      </Button>
                    </div>

                    <Tabs defaultValue="basic">
                      <TabsList>
                        <TabsTrigger value="basic">Основное</TabsTrigger>
                        <TabsTrigger value="variants">
                          Варианты ({taskType.variants.length})
                        </TabsTrigger>
                        <TabsTrigger value="grading">Оценивание</TabsTrigger>
                      </TabsList>

                      <TabsContent value="basic" className="space-y-4">
                        <div>
                          <Label>Название</Label>
                          <Input
                            value={taskType.title}
                            onChange={(e) =>
                              updateTaskType(taskIndex, "title", e.target.value)
                            }
                            placeholder="Расчет pH раствора"
                          />
                        </div>
                        <div>
                          <Label>Описание задачи</Label>
                          <Textarea
                            value={taskType.description}
                            onChange={(e) =>
                              updateTaskType(
                                taskIndex,
                                "description",
                                e.target.value
                              )
                            }
                            placeholder="Полное условие задачи..."
                            rows={4}
                          />
                        </div>
                        <div className="grid grid-cols-2 gap-4">
                          <div>
                            <Label>Максимальный балл</Label>
                            <Input
                              type="number"
                              value={taskType.max_score}
                              onChange={(e) =>
                                updateTaskType(
                                  taskIndex,
                                  "max_score",
                                  parseFloat(e.target.value) || 0
                                )
                              }
                            />
                          </div>
                          <div>
                            <Label>Сложность</Label>
                            <select
                              className="w-full border rounded-md p-2"
                              value={taskType.difficulty}
                              onChange={(e) =>
                                updateTaskType(
                                  taskIndex,
                                  "difficulty",
                                  e.target.value as any
                                )
                              }
                            >
                              <option value="easy">Легкая</option>
                              <option value="medium">Средняя</option>
                              <option value="hard">Сложная</option>
                            </select>
                          </div>
                        </div>
                      </TabsContent>

                      <TabsContent value="variants" className="space-y-4">
                        <Button
                          size="sm"
                          variant="outline"
                          onClick={() => addVariant(taskIndex)}
                        >
                          <Plus className="w-4 h-4 mr-2" />
                          Добавить вариант
                        </Button>

                        {taskType.variants.map((variant, variantIndex) => (
                          <Card key={variantIndex} className="p-4">
                            <div className="flex items-start justify-between mb-4">
                              <h4 className="font-semibold">
                                Вариант {variantIndex + 1}
                              </h4>
                              {taskType.variants.length > 1 && (
                                <Button
                                  variant="ghost"
                                  size="sm"
                                  onClick={() =>
                                    removeVariant(taskIndex, variantIndex)
                                  }
                                >
                                  <Trash2 className="w-4 h-4" />
                                </Button>
                              )}
                            </div>
                            <div className="space-y-3">
                              <div>
                                <Label>Текст варианта</Label>
                                <Textarea
                                  value={variant.content}
                                  onChange={(e) =>
                                    updateVariant(
                                      taskIndex,
                                      variantIndex,
                                      "content",
                                      e.target.value
                                    )
                                  }
                                  placeholder="Вычислите pH раствора при смешивании..."
                                  rows={3}
                                />
                              </div>
                              <div>
                                <Label>Эталонное решение</Label>
                                <Textarea
                                  value={variant.reference_solution}
                                  onChange={(e) =>
                                    updateVariant(
                                      taskIndex,
                                      variantIndex,
                                      "reference_solution",
                                      e.target.value
                                    )
                                  }
                                  placeholder="Пошаговое решение..."
                                  rows={3}
                                />
                              </div>
                              <div>
                                <Label>Правильный ответ</Label>
                                <Input
                                  value={variant.reference_answer}
                                  onChange={(e) =>
                                    updateVariant(
                                      taskIndex,
                                      variantIndex,
                                      "reference_answer",
                                      e.target.value
                                    )
                                  }
                                  placeholder="pH = 3.14"
                                />
                              </div>
                            </div>
                          </Card>
                        ))}
                      </TabsContent>

                      <TabsContent value="grading">
                        <div className="space-y-4">
                          <p className="text-sm text-muted-foreground">
                            Критерии оценивания настраиваются через JSON. По
                            умолчанию используются стандартные критерии.
                          </p>
                          <Textarea
                            value={JSON.stringify(taskType.rubric, null, 2)}
                            onChange={(e) => {
                              try {
                                const rubric = JSON.parse(e.target.value);
                                updateTaskType(taskIndex, "rubric", rubric);
                              } catch (error) {
                                // Invalid JSON, ignore
                              }
                            }}
                            rows={10}
                            className="font-mono text-sm"
                          />
                        </div>
                      </TabsContent>
                    </Tabs>
                  </Card>
                ))}
              </div>
            )}
          </Card>

          {/* Actions */}
          <div className="flex gap-4 justify-end">
            <Button variant="outline" onClick={() => navigate("/teacher")} disabled={loading}>
              Отмена
            </Button>
            {!isEditMode && (
              <Button
                variant="outline"
                onClick={() => handleSubmit(false)}
                disabled={loading}
              >
                <Save className="w-4 h-4 mr-2" />
                Сохранить черновик
              </Button>
            )}
            <Button onClick={() => handleSubmit(isEditMode ? false : true)} disabled={loading}>
              {isEditMode ? "Сохранить изменения" : "Опубликовать"}
            </Button>
          </div>
        </div>
      </div>
    </div>
  );
};

export default CreateExam;

