import { Link } from "react-router-dom";
import { Button } from "@/components/ui/button";
import { Card } from "@/components/ui/card";
import { Navbar } from "@/components/Navbar";
import { FlaskConical, Brain, CheckCircle, BarChart3, Upload, Shield } from "lucide-react";

const Landing = () => {
  return (
    <div className="min-h-screen bg-gradient-to-b from-background via-secondary/20 to-background">
      <Navbar />
      
      {/* Hero Section */}
      <section className="pt-32 pb-20 px-6">
        <div className="container mx-auto text-center max-w-4xl">
          <div className="inline-block mb-6 px-4 py-2 rounded-full bg-gradient-to-r from-primary/10 to-accent/10 border border-primary/20">
            <span className="text-sm font-medium bg-gradient-to-r from-primary to-accent bg-clip-text text-transparent">
              Платформа онлайн-контроля <span className="whitespace-nowrap">знаний по химии</span>
            </span>
          </div>
          
          <h1 className="text-3xl sm:text-4xl md:text-5xl lg:text-6xl font-bold mb-6 bg-gradient-to-r from-foreground via-primary to-accent bg-clip-text text-transparent leading-tight px-2">
            Автоматизация проверки контрольных работ по химии
          </h1>
          
          <p className="text-xl text-muted-foreground mb-8 leading-relaxed">
            Picrete использует искусственный интеллект для проверки решений студентов, 
            экономя время преподавателей и обеспечивая объективное оценивание
          </p>
          
          <div className="flex gap-4 justify-center flex-wrap">
            <Link to="/signup">
              <Button size="lg" className="text-lg px-8 shadow-elegant hover:shadow-glow transition-all duration-300">
                Начать бесплатно
              </Button>
            </Link>
            <Link to="/demo">
              <Button size="lg" variant="outline" className="text-lg px-8">
                Посмотреть демо
              </Button>
            </Link>
          </div>
        </div>
      </section>

      {/* Features Grid */}
      <section className="py-20 px-6">
        <div className="container mx-auto max-w-6xl">
          <h2 className="text-3xl font-bold text-center mb-12">Возможности платформы</h2>
          
          <div className="grid md:grid-cols-2 lg:grid-cols-3 gap-6">
            <Card className="p-6 hover:shadow-elegant transition-all duration-300 border-border/50 bg-gradient-card">
              <div className="w-12 h-12 rounded-xl bg-gradient-to-br from-primary to-accent flex items-center justify-center mb-4">
                <Brain className="w-6 h-6 text-white" />
              </div>
              <h3 className="text-xl font-semibold mb-2">AI-проверка</h3>
              <p className="text-muted-foreground">
                GPT-5 Thinking анализирует решения студентов с учётом критериев оценивания и химических правил
              </p>
            </Card>

            <Card className="p-6 hover:shadow-elegant transition-all duration-300 border-border/50 bg-gradient-card">
              <div className="w-12 h-12 rounded-xl bg-gradient-to-br from-primary to-accent flex items-center justify-center mb-4">
                <Upload className="w-6 h-6 text-white" />
              </div>
              <h3 className="text-xl font-semibold mb-2">Загрузка фото</h3>
              <p className="text-muted-foreground">
                Студенты фотографируют решения, система автоматически обрабатывает и распознаёт почерк
              </p>
            </Card>

            <Card className="p-6 hover:shadow-elegant transition-all duration-300 border-border/50 bg-gradient-card">
              <div className="w-12 h-12 rounded-xl bg-gradient-to-br from-primary to-accent flex items-center justify-center mb-4">
                <FlaskConical className="w-6 h-6 text-white" />
              </div>
              <h3 className="text-xl font-semibold mb-2">Химические правила</h3>
              <p className="text-muted-foreground">
                Проверка баланса реакций, размерностей, стехиометрии и других предметных требований
              </p>
            </Card>

            <Card className="p-6 hover:shadow-elegant transition-all duration-300 border-border/50 bg-gradient-card">
              <div className="w-12 h-12 rounded-xl bg-gradient-to-br from-primary to-accent flex items-center justify-center mb-4">
                <CheckCircle className="w-6 h-6 text-white" />
              </div>
              <h3 className="text-xl font-semibold mb-2">Верификация</h3>
              <p className="text-muted-foreground">
                Преподаватель утверждает или корректирует оценки AI, добавляет комментарии для студентов
              </p>
            </Card>

            <Card className="p-6 hover:shadow-elegant transition-all duration-300 border-border/50 bg-gradient-card">
              <div className="w-12 h-12 rounded-xl bg-gradient-to-br from-primary to-accent flex items-center justify-center mb-4">
                <BarChart3 className="w-6 h-6 text-white" />
              </div>
              <h3 className="text-xl font-semibold mb-2">Аналитика</h3>
              <p className="text-muted-foreground">
                Детальная статистика по успеваемости, частым ошибкам и прогрессу студентов
              </p>
            </Card>

            <Card className="p-6 hover:shadow-elegant transition-all duration-300 border-border/50 bg-gradient-card">
              <div className="w-12 h-12 rounded-xl bg-gradient-to-br from-primary to-accent flex items-center justify-center mb-4">
                <Shield className="w-6 h-6 text-white" />
              </div>
              <h3 className="text-xl font-semibold mb-2">Антиплагиат</h3>
              <p className="text-muted-foreground">
                Автоматическое обнаружение идентичных решений и подозрительных паттернов
              </p>
            </Card>
          </div>
        </div>
      </section>

      {/* CTA Section */}
      <section className="py-12 sm:py-16 md:py-20 px-4 sm:px-6">
        <div className="container mx-auto max-w-4xl">
          <Card className="p-6 sm:p-8 md:p-12 text-center bg-gradient-to-br from-primary via-accent to-primary border-0 shadow-glow">
            <h2 className="text-lg sm:text-2xl md:text-3xl lg:text-4xl font-bold text-white mb-4 sm:mb-6 px-1 sm:px-4 leading-tight break-words">
              <span className="block sm:inline">Готовы оптимизировать проверку&nbsp;контрольных?</span>
            </h2>
            <p className="text-base sm:text-lg md:text-xl text-white/90 mb-6 sm:mb-8 px-2 sm:px-4">
              Присоединяйтесь к преподавателям, которые экономят время с Picrete
            </p>
            <div className="flex justify-center">
              <Link to="/signup">
                <Button size="lg" variant="secondary" className="text-base sm:text-lg px-6 sm:px-8 bg-white hover:bg-white/90 text-primary w-full sm:w-auto">
                  Создать аккаунт
                </Button>
              </Link>
            </div>
          </Card>
        </div>
      </section>

      {/* Footer */}
      <footer className="py-12 px-6 border-t border-border">
        <div className="container mx-auto text-center text-muted-foreground">
          <p>© 2025 Picrete. Платформа автоматизированной проверки контрольных работ по химии</p>
        </div>
      </footer>
    </div>
  );
};

export default Landing;
