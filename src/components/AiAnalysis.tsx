import React from 'react';
import 'katex/dist/katex.min.css';
import { InlineMath, BlockMath } from 'react-katex';

type Criterion = {
  criterion_name?: string;
  score?: number;
  max_score?: number;
  comment?: string;
};

type AiAnalysisData = {
  total_score?: number | null;
  max_score?: number | null;
  criteria_scores?: Criterion[];
  method_correctness?: string;
  calculations?: string;
  units_and_dimensions?: string;
  chemical_rules?: string;
  errors_found?: string[];
  detailed_analysis?: Record<string, any> | string;
  feedback?: string;
  recommendations?: string[];
  [key: string]: any;
};

export default function AiAnalysis({ data }: { data: AiAnalysisData }) {
  if (!data) return null;

  const total = data.total_score ?? null;
  const max = data.max_score ?? null;
  const criteria = Array.isArray(data.criteria_scores) ? data.criteria_scores : [];
  const errors = Array.isArray(data.errors_found) ? data.errors_found : [];
  const recommendations = Array.isArray(data.recommendations) ? data.recommendations : [];

  return (
    <div className="space-y-6">
      {(total !== null || max !== null) && (
        <div>
          <h4 className="font-semibold mb-1">Сводка</h4>
          <p className="text-sm">Балл: {total ?? "—"}{max !== null ? ` / ${max}` : ""}</p>
        </div>
      )}

      {criteria.length > 0 && (
        <div>
          <h4 className="font-semibold mb-2">Критерии</h4>
          <div className="overflow-x-auto">
            <table className="w-full text-sm">
              <thead>
                <tr className="text-left text-muted-foreground border-b">
                  <th className="py-2 pr-4">Критерий</th>
                  <th className="py-2 pr-4">Балл</th>
                  <th className="py-2">Комментарий</th>
                </tr>
              </thead>
              <tbody>
                {criteria.map((c, i) => (
                  <tr key={i} className="border-b last:border-0 align-top">
                    <td className="py-2 pr-4 font-medium">{c.criterion_name || "—"}</td>
                    <td className="py-2 pr-4 whitespace-nowrap">{(c.score ?? "—")} / {(c.max_score ?? "—")}</td>
                    <td className="py-2">
                      <div className="whitespace-pre-wrap text-muted-foreground">{c.comment || "—"}</div>
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        </div>
      )}

      {(data.method_correctness || data.calculations || data.units_and_dimensions || data.chemical_rules) && (
        <div className="grid md:grid-cols-2 gap-4">
          {data.method_correctness && (
            <div>
              <h4 className="font-semibold mb-1">Корректность метода</h4>
              <p className="text-sm whitespace-pre-wrap">{data.method_correctness}</p>
            </div>
          )}
          {data.calculations && (
            <div>
              <h4 className="font-semibold mb-1">Вычисления</h4>
              <p className="text-sm whitespace-pre-wrap">{data.calculations}</p>
            </div>
          )}
          {data.units_and_dimensions && (
            <div>
              <h4 className="font-semibold mb-1">Размерности и единицы</h4>
              <p className="text-sm whitespace-pre-wrap">{data.units_and_dimensions}</p>
            </div>
          )}
          {data.chemical_rules && (
            <div>
              <h4 className="font-semibold mb-1">Химические правила</h4>
              <p className="text-sm whitespace-pre-wrap">{data.chemical_rules}</p>
            </div>
          )}
        </div>
      )}

      {errors.length > 0 && (
        <div>
          <h4 className="font-semibold mb-1 text-red-600">Найденные ошибки</h4>
          <ul className="list-disc list-inside text-sm">
            {errors.map((err, i) => (
              <li key={i}>{err}</li>
            ))}
          </ul>
        </div>
      )}

      {data.detailed_analysis && (
        <div>
          <h4 className="font-semibold mb-1">Подробный разбор</h4>
          {typeof data.detailed_analysis === "string" ? (
            <p className="text-sm whitespace-pre-wrap">{data.detailed_analysis}</p>
          ) : (
            <div className="space-y-2 text-sm">
              {Object.entries(data.detailed_analysis).map(([k, v]) => (
                <div key={k}>
                  <div className="font-medium">{k}</div>
                  <div className="text-muted-foreground whitespace-pre-wrap">{String(v)}</div>
                </div>
              ))}
            </div>
          )}
        </div>
      )}

      {data.feedback && (
        <div>
          <h4 className="font-semibold mb-1">Обратная связь</h4>
          <p className="text-sm whitespace-pre-wrap">{data.feedback}</p>
        </div>
      )}

      {recommendations.length > 0 && (
        <div>
          <h4 className="font-semibold mb-1">Рекомендации</h4>
          <ul className="list-disc list-inside text-sm">
            {recommendations.map((r, i) => (
              <li key={i}>{r}</li>
            ))}
          </ul>
        </div>
      )}
    </div>
  );
}


