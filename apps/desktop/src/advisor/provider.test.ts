import { describe, it, expect } from 'vitest';
import { detectProvider, isConfigured, parseFirstJson, extractAnthropicText, dataUrlBase64, isToolsUnsupportedError } from './provider';

describe('detectProvider', () => {
  it('defaults to openai for empty/unknown base urls', () => {
    expect(detectProvider('')).toBe('openai');
    expect(detectProvider('https://api.deepseek.com/v1')).toBe('openai');
  });

  it('detects ollama from localhost:11434 or /api/chat', () => {
    expect(detectProvider('http://localhost:11434')).toBe('ollama');
    expect(detectProvider('http://127.0.0.1:11434/api/chat')).toBe('ollama');
  });

  it('detects anthropic from proxy subdomains and /v1/messages', () => {
    expect(detectProvider('https://api.anthropic.com')).toBe('anthropic');
    expect(detectProvider('https://anthropic.novadiffusion.com')).toBe('anthropic');
    expect(detectProvider('https://x.com/v1/messages')).toBe('anthropic');
  });

  it('detects gemini from googleapis', () => {
    expect(detectProvider('https://generativelanguage.googleapis.com')).toBe('gemini');
    expect(detectProvider('https://generativelanguage.googleapis.com/v1beta')).toBe('gemini');
  });
});

describe('isConfigured', () => {
  it('requires key+model for keyed providers', () => {
    expect(isConfigured({ provider: 'openai', model: 'gpt-4o', apiKey: 'sk-x', baseUrl: '' })).toBe(true);
    expect(isConfigured({ provider: 'openai', model: 'gpt-4o', apiKey: '', baseUrl: '' })).toBe(false);
    expect(isConfigured(null)).toBe(false);
  });

  it('ollama only needs a model', () => {
    expect(isConfigured({ provider: 'ollama', model: 'qwen2.5', apiKey: '', baseUrl: 'http://localhost:11434' })).toBe(true);
    expect(isConfigured({ provider: 'ollama', model: '', apiKey: '', baseUrl: 'http://localhost:11434' })).toBe(false);
  });
});

describe('parseFirstJson', () => {
  it('parses a plain json string', () => {
    expect(parseFirstJson('{"a":1}')).toEqual({ a: 1 });
  });

  it('keeps only the first complete json value when trailing junk follows', () => {
    const s = '{"a":1}{"usage":{"x":2}}\n';
    expect(parseFirstJson(s)).toEqual({ a: 1 });
  });

  it('handles braces inside strings', () => {
    expect(parseFirstJson('{"msg":"hello {world}","n":2}')).toEqual({ msg: 'hello {world}', n: 2 });
  });

  it('parses arrays and throws on malformed', () => {
    expect(parseFirstJson('[1,2]')).toEqual([1, 2]);
    expect(() => parseFirstJson('{"a":')).toThrow();
  });
});

describe('extractAnthropicText', () => {
  it('joins all text blocks, skipping thinking blocks', () => {
    const data = {
      content: [
        { type: 'thinking', thinking: '...' },
        { type: 'text', text: '{"what":"a"}' },
        { type: 'text', text: '{"what":"b"}' },
      ],
    };
    expect(extractAnthropicText(data)).toBe('{"what":"a"}{"what":"b"}');
  });

  it('gives a clear error when max_tokens cut off thinking', () => {
    expect(() => extractAnthropicText({ content: [{ type: 'thinking' }], stop_reason: 'max_tokens' }))
      .toThrow(/thinking 阶段被截断/);
  });

  it('gives a clear error when no text block at all', () => {
    expect(() => extractAnthropicText({ content: [{ type: 'thinking' }], stop_reason: 'stop' }))
      .toThrow(/没拿到 text block/);
  });
});

describe('dataUrlBase64', () => {
  it('strips the data: prefix', () => {
    expect(dataUrlBase64('data:image/png;base64,AAAA')).toBe('AAAA');
  });

  it('returns input unchanged when no comma', () => {
    expect(dataUrlBase64('AAAA')).toBe('AAAA');
  });
});

describe('isToolsUnsupportedError', () => {
  it('accepts 400/422 with "not supported/enabled" tool wording', () => {
    expect(isToolsUnsupportedError('HTTP 400: \'tools\' is not supported by this model')).toBe(true);
    expect(isToolsUnsupportedError('HTTP 422: this model does not support tool use')).toBe(true);
    expect(isToolsUnsupportedError('HTTP 400: Function calling is not enabled for this model')).toBe(true);
  });

  it('rejects non-400/422 errors even with tool wording', () => {
    expect(isToolsUnsupportedError('HTTP 401: tools not authorized')).toBe(false);
    expect(isToolsUnsupportedError('HTTP 429: rate limited while calling tool')).toBe(false);
    expect(isToolsUnsupportedError('TypeError: Failed to fetch')).toBe(false);
  });

  it('rejects 400/422 without "not supported" wording (real arg errors)', () => {
    expect(isToolsUnsupportedError('HTTP 400: tool_call_id is required')).toBe(false);
    expect(isToolsUnsupportedError('HTTP 422: invalid tools parameter schema')).toBe(false);
    expect(isToolsUnsupportedError('HTTP 400: {"error":{"message":"invalid json"}}')).toBe(false);
  });
});
