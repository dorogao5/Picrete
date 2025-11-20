import React, { useState, useEffect, useCallback, useRef } from "react";
import { useParams, useNavigate } from "react-router-dom";
import { Navbar } from "@/components/Navbar";
import { Button } from "@/components/ui/button";
import { Card } from "@/components/ui/card";
import { Progress } from "@/components/ui/progress";
import { Alert, AlertDescription } from "@/components/ui/alert";
import { Dialog, DialogContent, DialogDescription, DialogHeader, DialogTitle } from "@/components/ui/dialog";
import { Clock, Upload, CheckCircle, AlertCircle, Image as ImageIcon } from "lucide-react";
import { submissionsAPI } from "@/lib/api";
import { toast } from "sonner";
import 'katex/dist/katex.min.css';
import { InlineMath, BlockMath } from 'react-katex';

const TakeExam = () => {
  const { examId } = useParams<{ examId: string }>();
  const navigate = useNavigate();
  
  const [session, setSession] = useState<any>(null);
  const [tasks, setTasks] = useState<any[]>([]);
  const [timeRemaining, setTimeRemaining] = useState(0);
  const [uploadedImages, setUploadedImages] = useState<{ [key: number]: File[] }>({});
  const [uploading, setUploading] = useState(false);
  const [submitting, setSubmitting] = useState(false);
  const [isTimeUp, setIsTimeUp] = useState(false);
  const [showTimeoutDialog, setShowTimeoutDialog] = useState(false);
  const initialRemainingRef = useRef<number | null>(null);

  // Save images to localStorage
  const saveImagesToStorage = useCallback(async (images: { [key: number]: File[] }) => {
    if (!session?.id) return;
    
    try {
      const storageKey = `exam_images_${session.id}`;
      const imageData: { [key: number]: { name: string; size: number; type: string; dataUrl: string }[] } = {};
      
      for (const taskIndex in images) {
        const taskImages = images[parseInt(taskIndex)];
        imageData[parseInt(taskIndex)] = [];
        
        for (const file of taskImages) {
          // Skip files larger than 2MB to avoid localStorage quota issues
          if (file.size > 2 * 1024 * 1024) {
            console.warn(`Skipping large file ${file.name} (${file.size} bytes) to avoid localStorage quota`);
            continue;
          }
          
          // Convert file to data URL for storage
          const dataUrl = await new Promise<string>((resolve) => {
            const reader = new FileReader();
            reader.onload = () => resolve(reader.result as string);
            reader.readAsDataURL(file);
          });
          
          imageData[parseInt(taskIndex)].push({
            name: file.name,
            size: file.size,
            type: file.type,
            dataUrl: dataUrl
          });
        }
      }
      
      const dataString = JSON.stringify(imageData);
      
      // Check localStorage quota (usually 5-10MB)
      if (dataString.length > 4 * 1024 * 1024) { // 4MB limit
        console.warn('Image data too large for localStorage, skipping save');
        return;
      }
      
      localStorage.setItem(storageKey, dataString);
    } catch (error) {
      console.warn('Failed to save images to localStorage:', error);
    }
  }, [session?.id]);

  // Load images from localStorage
  const loadImagesFromStorage = useCallback(async () => {
    if (!session?.id) return {};
    
    try {
      const storageKey = `exam_images_${session.id}`;
      const savedData = localStorage.getItem(storageKey);
      
      if (!savedData) return {};
      
      const imageData = JSON.parse(savedData);
      const loadedImages: { [key: number]: File[] } = {};
      
      for (const taskIndex in imageData) {
        const taskImages = imageData[parseInt(taskIndex)];
        loadedImages[parseInt(taskIndex)] = [];
        
        for (const imgData of taskImages) {
          try {
            // Convert data URL back to File
            const response = await fetch(imgData.dataUrl);
            const blob = await response.blob();
            const file = new File([blob], imgData.name, {
              type: imgData.type,
              lastModified: Date.now()
            });
            loadedImages[parseInt(taskIndex)].push(file);
          } catch (error) {
            console.warn('Failed to restore image:', imgData.name, error);
          }
        }
      }
      
      return loadedImages;
    } catch (error) {
      console.warn('Failed to load images from localStorage:', error);
      return {};
    }
  }, [session?.id]);

  // Helper functions
  const formatTime = (seconds: number) => {
    const hours = Math.floor(seconds / 3600);
    const minutes = Math.floor((seconds % 3600) / 60);
    const secs = Math.floor(seconds % 60);
    return `${hours.toString().padStart(2, "0")}:${minutes
      .toString()
      .padStart(2, "0")}:${secs.toString().padStart(2, "0")}`;
  };

  // Upload images function
  const uploadImages = useCallback(async () => {
    if (!session || isTimeUp) return;

    setUploading(true);
    try {
      let orderIndex = 0;
      for (const taskIndex in uploadedImages) {
        const files = uploadedImages[taskIndex];
        for (const file of files) {
          await submissionsAPI.uploadImage(session.id, file, orderIndex);
          orderIndex++;
        }
      }
      toast.success("Все изображения загружены");
    } catch (error: any) {
      toast.error(error.response?.data?.detail || "Ошибка при загрузке изображений");
    } finally {
      setUploading(false);
    }
  }, [session?.id, isTimeUp, uploadedImages]);

  // Auto-submit function
  const handleAutoSubmit = useCallback(async () => {
    if (!session || submitting) return;
    
    try {
      setSubmitting(true);
      
      // Upload any remaining images before submitting
      const totalImages = Object.values(uploadedImages).reduce(
        (sum, files) => sum + files.length,
        0
      );
      
      if (totalImages > 0) {
        await uploadImages();
      }
      
      // Submit exam - this will create submission if it doesn't exist
      await submissionsAPI.submit(session.id);
      
      // Clear saved images from localStorage
      if (session.id) {
        localStorage.removeItem(`exam_images_${session.id}`);
      }
      
      toast.success("Работа автоматически отправлена");
    } catch (error: any) {
      console.error("Auto-submit error:", error);
      const errorMessage = error.response?.data?.detail || "Ошибка при автоматической отправке работы";
      toast.error(errorMessage);
    } finally {
      setSubmitting(false);
      // Navigate to results page regardless of submission success
      // The backend scheduler will handle creating empty submissions for expired sessions
      navigate(`/exam/${session.id}/result`);
    }
  }, [session?.id, submitting, uploadedImages, uploadImages, navigate]);

  // Event handlers
  const handleTimeoutConfirm = useCallback(async () => {
    setShowTimeoutDialog(false);
    // Auto-submit already happened, just navigate
    if (session) {
      navigate(`/exam/${session.id}/result`);
    }
  }, [session?.id, navigate]);

  const handleImageSelect = useCallback((taskIndex: number, files: FileList | null) => {
    if (isTimeUp) {
      toast.error("Время экзамена истекло. Действия заблокированы.");
      return;
    }
    
    if (!files) return;

    const newFiles = Array.from(files).filter(
      (file) => file.type === "image/jpeg" || file.type === "image/png"
    );

    setUploadedImages((prev) => ({
      ...prev,
      [taskIndex]: [...(prev[taskIndex] || []), ...newFiles],
    }));

    toast.success(`Добавлено ${newFiles.length} изображений`);
  }, [isTimeUp]);

  const removeImage = useCallback((taskIndex: number, imageIndex: number) => {
    if (isTimeUp) {
      toast.error("Время экзамена истекло. Действия заблокированы.");
      return;
    }
    
    setUploadedImages((prev) => ({
      ...prev,
      [taskIndex]: prev[taskIndex].filter((_, i) => i !== imageIndex),
    }));
  }, [isTimeUp]);

  const handleSubmit = useCallback(async () => {
    if (!session || isTimeUp) return;

    // Check if images uploaded
    const totalImages = Object.values(uploadedImages).reduce(
      (sum, files) => sum + files.length,
      0
    );

    // Permit manual finish without images; backend will create empty submission if needed

    setSubmitting(true);
    try {
      // Upload remaining images
      if (totalImages > 0) {
        await uploadImages();
      }

      // Submit exam - this will create submission if it doesn't exist
      await submissionsAPI.submit(session.id);
      
      // Clear saved images from localStorage
      if (session.id) {
        localStorage.removeItem(`exam_images_${session.id}`);
      }
      
      toast.success("Работа отправлена на проверку");
      navigate(`/exam/${session.id}/result`);
    } catch (error: any) {
      console.error("Submit error:", error);
      const errorMessage = error.response?.data?.detail || "Ошибка при отправке работы";
      toast.error(errorMessage);
    } finally {
      setSubmitting(false);
    }
  }, [session?.id, isTimeUp, uploadedImages, uploadImages, navigate]);

  const renderLatex = useCallback((text: string) => {
    // Split by newlines first
    const lines = text.split('\n');
    
    return lines.map((line, lineIndex) => {
      // Process each line for LaTeX
      const parts = line.split(/(\$\$[\s\S]+?\$\$|\$[\s\S]+?\$)/);
      
      const lineContent = parts.map((part, partIndex) => {
        if (part.startsWith("$$") && part.endsWith("$$")) {
          return <BlockMath key={partIndex}>{part.slice(2, -2)}</BlockMath>;
        } else if (part.startsWith("$") && part.endsWith("$")) {
          return <InlineMath key={partIndex}>{part.slice(1, -1)}</InlineMath>;
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
  }, []);

  // Load session and variant
  useEffect(() => {
    const enterExam = async () => {
      try {
        const response = await submissionsAPI.enterExam(examId!);
        const sessionData = response.data;
        setSession(sessionData);
        
        // Load variant
        const variantResponse = await submissionsAPI.getSessionVariant(sessionData.id);
        setTasks(variantResponse.data.tasks);
        setTimeRemaining(variantResponse.data.time_remaining);
        initialRemainingRef.current = variantResponse.data.time_remaining;
        
        // Load saved images from localStorage
        const savedImages = await loadImagesFromStorage();
        if (Object.keys(savedImages).length > 0) {
          setUploadedImages(savedImages);
          const totalImages = Object.values(savedImages).reduce((sum, files) => sum + files.length, 0);
          toast.info(`Восстановлено ${totalImages} ранее загруженных изображений`);
        }
      } catch (error: any) {
        // Don't show error toast or navigate if it's an auth error
        // The axios interceptor will handle redirecting to login
        if (error.response?.status !== 401) {
          toast.error(error.response?.data?.detail || "Ошибка при входе в экзамен");
          navigate("/student");
        }
      }
    };

    if (examId) {
      enterExam();
    }
  }, [examId, navigate, loadImagesFromStorage]);

  // Timer countdown with auto-submit
  useEffect(() => {
    if (timeRemaining <= 0 || isTimeUp) {
      return;
    }
    const interval = setInterval(() => {
      setTimeRemaining((prev) => {
        if (prev <= 1) {
          // Auto-submit 1 second before time expires
          handleAutoSubmit();
          setIsTimeUp(true);
          setShowTimeoutDialog(true);
          return 0;
        }
        return prev - 1;
      });
    }, 1000);
    return () => clearInterval(interval);
  }, [timeRemaining, isTimeUp, handleAutoSubmit]);

  // Auto-save images to localStorage when they change
  useEffect(() => {
    if (session?.id && Object.keys(uploadedImages).length > 0) {
      saveImagesToStorage(uploadedImages);
    }
  }, [uploadedImages, session?.id, saveImagesToStorage]);

  // Periodic auto-save every 10 seconds
  useEffect(() => {
    if (!session?.id || isTimeUp) return;
    
    const interval = setInterval(() => {
      if (Object.keys(uploadedImages).length > 0) {
        saveImagesToStorage(uploadedImages);
        console.log('Auto-saved images to localStorage');
      }
    }, 10000); // Every 10 seconds
    
    return () => clearInterval(interval);
  }, [session?.id, uploadedImages, saveImagesToStorage, isTimeUp]);

  // Cleanup localStorage only when exam is submitted or session ends
  useEffect(() => {
    return () => {
      // Only clean up localStorage if exam was submitted or time is up
      if (session?.id && (isTimeUp || submitting)) {
        localStorage.removeItem(`exam_images_${session.id}`);
      }
    };
  }, [session?.id, isTimeUp, submitting]);

  if (!session || tasks.length === 0) {
    return (
      <div className="min-h-screen bg-gradient-subtle">
        <Navbar />
        <div className="container mx-auto px-6 pt-24 pb-12">
          <p>Загрузка экзамена...</p>
        </div>
      </div>
    );
  }

  const total = initialRemainingRef.current ?? timeRemaining;
  const progressPercent = total > 0 ? (timeRemaining / total) * 100 : 0;

  return (
    <div className="min-h-screen bg-gradient-subtle">
      <Navbar />

      {/* Timer Bar */}
      <div className="fixed top-16 left-0 right-0 z-40 bg-background border-b shadow-md">
        <div className="container mx-auto px-6 py-4">
          <div className="flex items-center justify-between mb-2">
            <div className="flex items-center gap-2">
              <Clock className="w-5 h-5" />
              <span className="font-semibold">Оставшееся время:</span>
              <span
                className={`text-2xl font-mono ${
                  timeRemaining < 600 ? "text-red-500" : "text-primary"
                }`}
              >
                {formatTime(timeRemaining)}
              </span>
            </div>
            <Button
              onClick={handleSubmit}
              disabled={submitting || uploading || isTimeUp}
              variant="default"
            >
              <CheckCircle className="w-4 h-4 mr-2" />
              {isTimeUp ? "Время истекло" : "Завершить работу"}
            </Button>
          </div>
          <Progress value={progressPercent} className="h-2" />
        </div>
      </div>

      <div className="container mx-auto px-6 pt-40 pb-12">
        {/* Warning if low time */}
        {timeRemaining < 600 && timeRemaining > 0 && !isTimeUp && (
          <Alert className="mb-6 border-red-500">
            <AlertCircle className="h-4 w-4" />
            <AlertDescription>
              Осталось менее 10 минут! Не забудьте завершить работу.
            </AlertDescription>
          </Alert>
        )}

        {/* Time up warning */}
        {isTimeUp && (
          <Alert className="mb-6 border-red-500 bg-red-50 dark:bg-red-950">
            <AlertCircle className="h-4 w-4" />
            <AlertDescription>
              Время экзамена истекло! Все действия заблокированы. Работа будет автоматически отправлена.
            </AlertDescription>
          </Alert>
        )}

        {/* Tasks */}
        <div className="space-y-8">
          {tasks.map((task, index) => (
            <Card key={task.task_type.id} className="p-6">
              <div className="mb-4">
                <div className="flex items-center justify-between mb-2">
                  <h2 className="text-2xl font-bold">
                    Задача {index + 1}. {task.task_type.title}
                  </h2>
                  <span className="text-sm font-semibold px-3 py-1 rounded-full bg-primary/10 text-primary">
                    {task.task_type.max_score} баллов
                  </span>
                </div>
                
                <div className="prose max-w-none">
                  <p className="text-muted-foreground mb-4">
                    {renderLatex(task.task_type.description)}
                  </p>
                  
                  <div className="bg-secondary/50 p-4 rounded-lg mb-4">
                    <h3 className="font-semibold mb-2">Ваш вариант:</h3>
                    <div>{renderLatex(task.variant.content)}</div>
                  </div>
                </div>

                {/* Formulas reference */}
                {task.task_type.formulas.length > 0 && (
                  <div className="bg-blue-50 dark:bg-blue-950 p-4 rounded-lg mb-4">
                    <h4 className="font-semibold mb-2">Формулы:</h4>
                    <div className="space-y-1">
                      {task.task_type.formulas.map((formula: string, i: number) => (
                        <div key={i}>{renderLatex(formula)}</div>
                      ))}
                    </div>
                  </div>
                )}
              </div>

              {/* Image Upload */}
              <div className="border-t pt-4">
                <h3 className="font-semibold mb-3 flex items-center gap-2">
                  <ImageIcon className="w-5 h-5" />
                  Загрузите фото решения
                </h3>

                <div className="mb-4">
                  <label className="block">
                    <div className={`border-2 border-dashed rounded-lg p-6 text-center transition-colors ${
                      isTimeUp 
                        ? 'cursor-not-allowed opacity-50 bg-gray-100 dark:bg-gray-800' 
                        : 'cursor-pointer hover:border-primary'
                    }`}>
                      <Upload className="w-8 h-8 mx-auto mb-2 text-muted-foreground" />
                      <p className="text-sm text-muted-foreground">
                        {isTimeUp 
                          ? "Время истекло - загрузка заблокирована" 
                          : "Нажмите или перетащите изображения (JPEG, PNG)"
                        }
                      </p>
                      <input
                        type="file"
                        multiple
                        accept="image/jpeg,image/png"
                        onChange={(e) => handleImageSelect(index, e.target.files)}
                        disabled={isTimeUp}
                        className="hidden"
                      />
                    </div>
                  </label>
                </div>

                {/* Preview uploaded images */}
                {uploadedImages[index] && uploadedImages[index].length > 0 && (
                  <div className="grid grid-cols-2 md:grid-cols-4 gap-4">
                    {uploadedImages[index].map((file, imgIndex) => (
                      <div
                        key={imgIndex}
                        className="relative border rounded-lg overflow-hidden group"
                      >
                        <img
                          src={URL.createObjectURL(file)}
                          alt={`Uploaded ${imgIndex + 1}`}
                          className="w-full h-32 object-cover"
                        />
                        <Button
                          size="sm"
                          variant="destructive"
                          className="absolute top-2 right-2 opacity-0 group-hover:opacity-100 transition-opacity"
                          onClick={() => removeImage(index, imgIndex)}
                          disabled={isTimeUp}
                        >
                          Удалить
                        </Button>
                        <div className="absolute bottom-0 left-0 right-0 bg-black/50 text-white text-xs p-1 text-center">
                          {file.name}
                        </div>
                      </div>
                    ))}
                  </div>
                )}
              </div>
            </Card>
          ))}
        </div>

        {/* Auto-save indicator */}
        <div className="fixed bottom-6 right-6">
          <Card className="p-3 shadow-lg">
            <p className="text-xs text-muted-foreground">
              <CheckCircle className="w-3 h-3 inline mr-1" />
              Автосохранение активно
            </p>
          </Card>
        </div>
      </div>

      {/* Timeout Dialog */}
      <Dialog open={showTimeoutDialog} onOpenChange={setShowTimeoutDialog}>
        <DialogContent className="sm:max-w-md">
          <DialogHeader>
            <DialogTitle className="flex items-center gap-2">
              <Clock className="w-5 h-5 text-red-500" />
              Время истекло
            </DialogTitle>
            <DialogDescription>
              Время экзамена закончилось. Работа будет автоматически отправлена на проверку.
            </DialogDescription>
          </DialogHeader>
          <div className="flex justify-end gap-2 mt-4">
            <Button onClick={handleTimeoutConfirm} className="w-full">
              Понятно
            </Button>
          </div>
        </DialogContent>
      </Dialog>
    </div>
  );
};

export default TakeExam;

