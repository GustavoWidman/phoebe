import { describe, expect, test } from 'vitest';
import { otherDay } from './format.ts';

// moments around noon UTC, which fall on the same day in every time zone a test runs in
const at = (iso: string) => Date.parse(iso);

describe('otherDay', () => {
	test('is nothing on the day of now, and the day and month on any other', () => {
		const now = at('2026-11-25T12:00:00Z');
		expect(otherDay(at('2026-11-25T11:00:00Z'), now)).toBeNull();
		expect(otherDay(at('2026-11-23T12:00:00Z'), now)).toBe('23 Nov');
		expect(otherDay(at('2026-10-31T12:00:00Z'), now)).toBe('31 Oct');
	});
});
