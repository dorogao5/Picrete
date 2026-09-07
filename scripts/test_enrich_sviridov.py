import csv
import json
from pathlib import Path
import tempfile
import unittest

from enrich_sviridov import merge, readable_math


class EnrichSviridovTests(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.root = Path(self.tmp.name)
        self.bank = self.root / 'bank'
        self.csv = self.root / 'csv' / 'Химическая термодинамика'
        self.bank.mkdir()
        self.csv.mkdir(parents=True)
        (self.bank / 'ocr_output/Sviridov_tasks').mkdir(parents=True)
        self.original = [{'paragraph': '7', 'topic': 'Thermodynamics', 'theory_text': 'Theory', 'tasks': [
            {'number': '7.1', 'text': 'Old text', 'answer': '42', 'images': []},
            {'number': '7.3', 'text': 'Untouched', 'answer': '', 'images': []},
        ]}]
        (self.bank / 'Sviridov_tasks.json').write_text(json.dumps(self.original))

    def row(self, number='7.1', **overrides):
        return {'номер': number, 'задание': 'Calculate', 'решение': 'A detailed derivation', 'тип': 'расчетное', 'сложность': 'средняя', 'объем': 'среднее', **overrides}

    def write_rows(self, rows):
        with (self.csv / 'data.csv').open('w', newline='') as stream:
            writer = csv.DictWriter(stream, fieldnames=list(self.row()))
            writer.writeheader()
            writer.writerows(rows)

    def test_merge_preserves_answers_unrelated_tasks_and_is_repeatable(self):
        self.write_rows([self.row(), self.row('7.2'), self.row('7.2')])
        data, copies, report = merge(self.root / 'csv', self.bank)
        self.assertEqual(report['added'], ['7.2'])
        self.assertEqual(report['duplicates'], ['7.2'])
        self.assertEqual(data[0]['tasks'][0]['answer'], '42')
        self.assertEqual(data[0]['tasks'][2], self.original[0]['tasks'][1])
        self.assertFalse(copies)
        (self.bank / 'Sviridov_tasks.json').write_text(json.dumps(data))
        repeated, _, _ = merge(self.root / 'csv', self.bank)
        self.assertEqual(data, repeated)

    def test_conflicting_duplicates_fail_before_writes(self):
        self.write_rows([self.row(), self.row(решение='Conflicting solution')])
        with self.assertRaisesRegex(ValueError, 'Conflicting duplicate'):
            merge(self.root / 'csv', self.bank)
        self.assertEqual(json.loads((self.bank / 'Sviridov_tasks.json').read_text()), self.original)

    def test_missing_image_and_unknown_labels_fail(self):
        self.write_rows([self.row(задание='![figure](missing.png)')])
        with self.assertRaisesRegex(ValueError, 'missing or unsafe image'):
            merge(self.root / 'csv', self.bank)
        self.write_rows([self.row(сложность='guess')])
        with self.assertRaisesRegex(ValueError, 'unknown сложность'):
            merge(self.root / 'csv', self.bank)

    def test_prose_wraps_but_formula_groups_and_script_bases_stay_in_math(self):
        result = readable_math(r'$\text{Длинное объяснение закона для H}_{2}\text{O }=\text{ вода}$')
        self.assertEqual(result, r'Длинное объяснение закона для $\text{H}_{2}\text{O }=\text{ вода}$')
        nested = r'$\frac{\text{Длинный текст внутри числителя}}{2}$'
        self.assertEqual(readable_math(nested), nested)
        self.assertIn(r'\int', readable_math(r'$1+\text{ ∫(}x)$'))


if __name__ == '__main__':
    unittest.main()
