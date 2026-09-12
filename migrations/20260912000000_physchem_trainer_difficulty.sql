-- The physical-chemistry examples are introductory exercises.  The old
-- Studio bridge assigned every imported item and every trainer slot `hard`,
-- which made the student UI show the wrong level for all five sections.
-- Keep this data correction in the Git release so it is reproducible on every
-- production instance; do not repair the live JSON manually.

CREATE OR REPLACE FUNCTION _picrete_physchem_easy_sections(document jsonb)
RETURNS jsonb
LANGUAGE sql
IMMUTABLE
AS $$
    SELECT jsonb_set(
        document,
        '{sections}',
        COALESCE(
            jsonb_agg(
                jsonb_set(
                    section,
                    '{items}',
                    COALESCE(
                        (
                            SELECT jsonb_agg(
                                jsonb_set(item, '{difficulty}', to_jsonb('easy'::text), true)
                                ORDER BY item->>'task_id'
                            )
                            FROM jsonb_array_elements(COALESCE(section->'items', '[]'::jsonb)) AS item
                        ),
                        '[]'::jsonb
                    ),
                    true
                )
                ORDER BY section->>'id'
            ),
            '[]'::jsonb
        ),
        true
    )
    FROM jsonb_array_elements(COALESCE(document->'sections', '[]'::jsonb)) AS section;
$$;

UPDATE task_bank_items AS item
SET difficulty = 'easy',
    volume = COALESCE(NULLIF(item.volume, ''), 'medium')
FROM task_bank_sources AS source
JOIN course_task_bank_sources AS binding ON binding.source_id = source.id
WHERE item.source_id = source.id
  AND source.code = 'studio_fizicheskaya_himiya';

UPDATE course_trainers AS trainer
SET draft = _picrete_physchem_easy_sections(trainer.draft),
    published = CASE
        WHEN trainer.published IS NULL THEN NULL
        ELSE _picrete_physchem_easy_sections(trainer.published)
    END,
    revision = trainer.revision + 1,
    updated_at = now()
WHERE EXISTS (
    SELECT 1
    FROM course_task_bank_sources AS binding
    JOIN task_bank_sources AS source ON source.id = binding.source_id
    JOIN courses AS course ON course.id = binding.course_id
    WHERE binding.course_id = trainer.course_id
      AND source.code = 'studio_fizicheskaya_himiya'
);

DROP FUNCTION _picrete_physchem_easy_sections(jsonb);
