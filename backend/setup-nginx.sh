#!/bin/bash
# Скрипт для настройки Nginx (HTTP конфигурация для получения SSL)

set -e

echo "=== Настройка Nginx для Picrete ==="

# Проверка прав root
if [ "$EUID" -ne 0 ]; then 
    echo "ОШИБКА: Пожалуйста, запустите скрипт с sudo"
    exit 1
fi

# Установка nginx, если не установлен
if ! command -v nginx &> /dev/null; then
    echo "Установка Nginx..."
    apt-get update
    apt-get install -y nginx
fi

# Создание директории для Let's Encrypt challenges
echo "Создание директории для Let's Encrypt..."
mkdir -p /var/www/html
chown -R www-data:www-data /var/www/html
chmod -R 755 /var/www/html

# Удаление дефолтной конфигурации
echo "Удаление дефолтной конфигурации nginx..."
rm -f /etc/nginx/sites-enabled/default

# Создание базовой конфигурации для HTTP (до получения SSL)
echo "Создание HTTP конфигурации для Picrete..."
cat > /etc/nginx/sites-available/picrete << 'EOF'
# Временная HTTP конфигурация (для получения SSL сертификата через certbot)
# После получения SSL certbot автоматически обновит эту конфигурацию

server {
    listen 80;
    listen [::]:80;
    server_name picrete.com www.picrete.com localhost;
    
    # Для Let's Encrypt certbot
    location /.well-known/acme-challenge/ {
        root /var/www/html;
    }
    
    # Размер загружаемых файлов
    client_max_body_size 20M;
    client_body_buffer_size 128k;
    
    # Backend API
    location /api/v1 {
        proxy_pass http://127.0.0.1:8000;
        proxy_http_version 1.1;
        
        proxy_set_header Host $host;
        proxy_set_header X-Real-IP $remote_addr;
        proxy_set_header X-Forwarded-For $proxy_add_x_forwarded_for;
        proxy_set_header X-Forwarded-Proto $scheme;
        proxy_set_header X-Forwarded-Host $host;
        proxy_set_header X-Forwarded-Port $server_port;
        proxy_set_header Connection "";
        
        proxy_connect_timeout 60s;
        proxy_send_timeout 60s;
        proxy_read_timeout 60s;
        
        proxy_buffering off;
        proxy_request_buffering off;
    }
    
    # Health check
    location /healthz {
        proxy_pass http://127.0.0.1:8000/healthz;
        proxy_http_version 1.1;
        proxy_set_header Host $host;
        proxy_set_header X-Real-IP $remote_addr;
        proxy_set_header X-Forwarded-For $proxy_add_x_forwarded_for;
        proxy_set_header X-Forwarded-Proto $scheme;
    }
    
    # API документация (Swagger)
    location /api/v1/docs {
        proxy_pass http://127.0.0.1:8000/api/v1/docs;
        proxy_http_version 1.1;
        proxy_set_header Host $host;
        proxy_set_header X-Real-IP $remote_addr;
        proxy_set_header X-Forwarded-For $proxy_add_x_forwarded_for;
        proxy_set_header X-Forwarded-Proto $scheme;
    }
    
    location /api/v1/openapi.json {
        proxy_pass http://127.0.0.1:8000/api/v1/openapi.json;
        proxy_http_version 1.1;
        proxy_set_header Host $host;
        proxy_set_header X-Real-IP $remote_addr;
        proxy_set_header X-Forwarded-For $proxy_add_x_forwarded_for;
        proxy_set_header X-Forwarded-Proto $scheme;
    }
    
    # Frontend
    location / {
        root /srv/picrete/landing;
        try_files $uri $uri/ /index.html;
        index index.html;
    }
    
    access_log /var/log/nginx/picrete_access.log;
    error_log /var/log/nginx/picrete_error.log warn;
}
EOF

# Включение конфигурации
echo "Включение конфигурации..."
ln -sf /etc/nginx/sites-available/picrete /etc/nginx/sites-enabled/

# Проверка конфигурации
echo "Проверка конфигурации nginx..."
if ! nginx -t; then
    echo "ОШИБКА: Конфигурация nginx неверна!"
    exit 1
fi

# Перезапуск nginx
echo "Перезапуск nginx..."
systemctl restart nginx
systemctl enable nginx

# Проверка статуса
if systemctl is-active --quiet nginx; then
    echo "✓ Nginx успешно запущен"
else
    echo "ОШИБКА: Nginx не запустился!"
    exit 1
fi

echo ""
echo "=== Настройка завершена! ==="
echo ""
echo "✓ Nginx настроен и запущен"
echo "✓ HTTP работает на порту 80"
echo "✓ Готов к получению SSL сертификата"
echo ""
echo "Следующий шаг: получение SSL сертификата Let's Encrypt"
echo "  sudo apt-get install -y certbot python3-certbot-nginx"
echo "  sudo certbot --nginx -d picrete.com -d www.picrete.com"
echo ""
echo "ВАЖНО: После получения сертификата certbot автоматически обновит"
echo "конфигурацию nginx для работы с HTTPS и добавит редирект с HTTP на HTTPS."
echo ""

