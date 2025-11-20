"""AI Grading Service using OpenAI GPT"""
from openai import AsyncOpenAI
from typing import Dict, Any, List
import json
import logging
import httpx
import time
from datetime import datetime

from app.core.config import settings

logger = logging.getLogger(__name__)

# Configure detailed logging for AI requests
ai_logger = logging.getLogger("ai_grading")
ai_logger.setLevel(logging.INFO)

# Configure OpenAI client with proxy settings
# All requests will be routed through 188.213.0.226:8082 proxy server
client = AsyncOpenAI(
    api_key=settings.OPENAI_API_KEY,
    base_url=settings.OPENAI_BASE_URL,
    timeout=httpx.Timeout(
        timeout=settings.AI_REQUEST_TIMEOUT,
        connect=30.0,  # Connection timeout
        read=settings.AI_REQUEST_TIMEOUT,  # Read timeout
        write=30.0,  # Write timeout
        pool=10.0  # Pool timeout
    ),
    max_retries=3,  # Retry failed requests up to 3 times for reliability
)


GRADING_SYSTEM_PROMPT = """Вы — эксперт по химии и опытный преподаватель. 
Ваша задача — проверить решение студента по контрольной работе и выставить баллы согласно критериям.

ВАЖНО: Если вы не можете распознать текст на изображении или изображение нечитаемо, 
ВЫ ОБЯЗАНЫ вернуть предупреждение с флагом "unreadable": true и подробным описанием проблемы.

Критерии оценивания:
1. Корректность метода решения
2. Правильность вычислений
3. Соблюдение размерностей и единиц измерения
4. Правильная запись ответа
5. Обоснование решения

Правила химии:
- Проверка баланса химических реакций
- Проверка валентностей и зарядов
- Стехиометрические расчеты
- Перевод единиц измерения
- Округление по значащим цифрам
- Проверка формул органических соединений по ИЮПАК

Формат ответа (строгий JSON):
{
  "unreadable": false,
  "unreadable_reason": null,
  "total_score": <число>,
  "max_score": <число>,
  "criteria_scores": [
    {
      "criterion_name": "название критерия",
      "score": <число>,
      "max_score": <число>,
      "comment": "комментарий"
    }
  ],
  "detailed_analysis": {
    "method_correctness": "анализ метода",
    "calculations": "анализ вычислений",
    "units_and_dimensions": "анализ размерностей",
    "chemical_rules": "проверка химических правил",
    "errors_found": ["список ошибок"]
  },
  "feedback": "Общий фидбек для студента с рекомендациями",
  "recommendations": ["рекомендация 1", "рекомендация 2"],
  "full_transcription_md": "ПОЛНАЯ расшифровка решения студента без ИСПРАВЛЕНИЙ, в Markdown c LaTeX ($ ... $)",
  "per_page_transcriptions": ["строго посимвольная md+LaTeX расшифровка для страницы 1", "... для страницы 2", "..." ]
}
"""


async def grade_submission(
    images: List[str],  # base64 encoded images or URLs
    task_description: str,
    reference_solution: str,
    rubric: Dict[str, Any],
    max_score: float,
    chemistry_rules: Dict[str, Any] = None,
    submission_id: str = None
) -> Dict[str, Any]:
    """
    Grade a student submission using AI
    
    Args:
        images: List of image URLs or base64 encoded images
        task_description: The task/problem description
        reference_solution: Reference solution for comparison
        rubric: Grading rubric with criteria
        max_score: Maximum possible score
        chemistry_rules: Specific chemistry validation rules
        submission_id: Submission ID for logging purposes
        
    Returns:
        Grading result with scores and feedback
    """
    request_start_time = time.time()
    request_start_datetime = datetime.utcnow()
    
    ai_logger.info(f"[{submission_id}] Starting AI grading request at {request_start_datetime}")
    ai_logger.info(f"[{submission_id}] Images count: {len(images)}, Max score: {max_score}")
    
    try:
        # Prepare user prompt
        user_prompt = f"""
Задача:
{task_description}

Эталонное решение:
{reference_solution}

Критерии оценивания (максимум {max_score} баллов):
{json.dumps(rubric, ensure_ascii=False, indent=2)}

Правила проверки:
{json.dumps(chemistry_rules or {}, ensure_ascii=False, indent=2)}

Проанализируйте решение студента на изображениях и выставите баллы согласно критериям.
ОБЯЗАТЕЛЬНО используйте JSON формат ответа как описано в системном промпте.
"""
        
        # Prepare messages for GPT
        messages = [
            {"role": "system", "content": GRADING_SYSTEM_PROMPT},
            {
                "role": "user",
                "content": [
                    {"type": "text", "text": user_prompt}
                ]
            }
        ]
        
        # Add images to the message
        for image in images:
            if image.startswith("http"):
                messages[1]["content"].append({
                    "type": "image_url",
                    "image_url": {"url": image}
                })
            else:
                # Assume base64
                messages[1]["content"].append({
                    "type": "image_url",
                    "image_url": {"url": f"data:image/jpeg;base64,{image}"}
                })
        
        # Call OpenAI API through proxy server (will use GPT-5 when available)
        # All requests are routed through 188.213.0.226:8082 with automatic retries
        ai_logger.info(f"[{submission_id}] Sending request to OpenAI API (model: {settings.AI_MODEL}, endpoint: {settings.OPENAI_BASE_URL})")
        
        api_call_start = time.time()
        response = await client.chat.completions.create(
            model=settings.AI_MODEL,
            messages=messages,
            max_completion_tokens=settings.AI_MAX_TOKENS,
            response_format={"type": "json_object"}
        )
        api_call_duration = time.time() - api_call_start
        
        ai_logger.info(f"[{submission_id}] Received response from OpenAI API in {api_call_duration:.2f}s")
        ai_logger.info(f"[{submission_id}] Response tokens: {response.usage.total_tokens if response.usage else 'N/A'}")
        
        # Parse response
        result = json.loads(response.choices[0].message.content)
        
        request_end_time = time.time()
        total_duration = request_end_time - request_start_time
        ai_logger.info(f"[{submission_id}] Total AI grading completed in {total_duration:.2f}s")
        
        # Add timing metadata to result
        result["_metadata"] = {
            "request_started_at": request_start_datetime.isoformat(),
            "request_completed_at": datetime.utcnow().isoformat(),
            "duration_seconds": total_duration,
            "api_call_duration_seconds": api_call_duration,
            "tokens_used": response.usage.total_tokens if response.usage else None,
            "model": settings.AI_MODEL
        }
        
        # Validate result structure
        if "unreadable" not in result:
            result["unreadable"] = False
        
        if result["unreadable"]:
            ai_logger.warning(f"[{submission_id}] Submission marked as unreadable: {result.get('unreadable_reason', 'No reason provided')}")
            return {
                "unreadable": True,
                "unreadable_reason": result.get("unreadable_reason", "Изображение нечитаемо"),
                "total_score": None,
                "max_score": max_score,
                "requires_manual_review": True,
                "_metadata": result.get("_metadata", {})
            }
        
        ai_logger.info(f"[{submission_id}] AI grading successful. Score: {result.get('total_score')}/{max_score}")
        return result
        
    except json.JSONDecodeError as e:
        request_end_time = time.time()
        total_duration = request_end_time - request_start_time
        error_msg = f"Failed to parse AI response: {e}"
        ai_logger.error(f"[{submission_id}] {error_msg} (duration: {total_duration:.2f}s)")
        return {
            "error": error_msg,
            "total_score": None,
            "max_score": max_score,
            "requires_manual_review": True,
            "_metadata": {
                "request_started_at": request_start_datetime.isoformat(),
                "request_completed_at": datetime.utcnow().isoformat(),
                "duration_seconds": total_duration,
                "error": error_msg
            }
        }
    
    except Exception as e:
        request_end_time = time.time()
        total_duration = request_end_time - request_start_time
        error_msg = f"AI grading failed: {str(e)}"
        ai_logger.error(f"[{submission_id}] {error_msg} (duration: {total_duration:.2f}s)", exc_info=True)
        return {
            "error": error_msg,
            "total_score": None,
            "max_score": max_score,
            "requires_manual_review": True,
            "_metadata": {
                "request_started_at": request_start_datetime.isoformat(),
                "request_completed_at": datetime.utcnow().isoformat(),
                "duration_seconds": total_duration,
                "error": error_msg
            }
        }


async def validate_chemistry_rules(
    solution_data: Dict[str, Any],
    rules: Dict[str, Any]
) -> Dict[str, Any]:
    """
    Validate chemistry-specific rules
    
    Args:
        solution_data: Parsed solution data
        rules: Chemistry validation rules
        
    Returns:
        Validation results
    """
    validation_results = {
        "valid": True,
        "errors": [],
        "warnings": []
    }
    
    # TODO: Implement specific chemistry validation
    # - Balance equations
    # - Check dimensions
    # - Validate IUPAC names
    # - Check significant figures
    # - Validate units conversion
    
    return validation_results


async def detect_plagiarism(
    submission_images: List[str],
    other_submissions: List[Dict[str, Any]]
) -> Dict[str, Any]:
    """
    Detect potential plagiarism using perceptual hashing
    
    Args:
        submission_images: Images from current submission
        other_submissions: Other submissions to compare against
        
    Returns:
        Plagiarism detection results
    """
    # TODO: Implement perceptual hashing and comparison
    return {
        "is_suspicious": False,
        "similarity_scores": [],
        "matched_submissions": []
    }


