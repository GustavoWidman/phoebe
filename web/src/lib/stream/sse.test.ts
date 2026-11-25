import { expect, test } from 'vitest';
import { split } from './sse.ts';

test('split keeps a message cut in half for the next read', () => {
	const { messages, rest } = split('event: snapshot\ndata: {"a":1}\n\nevent: snap');
	expect(messages).toEqual(['{"a":1}']);
	expect(rest).toBe('event: snap');
});

test('split skips comments and keep-alives, and joins data lines', () => {
	expect(split(':\n\ndata: a\ndata: b\r\n\r\n').messages).toEqual(['a\nb']);
});
