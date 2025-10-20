import '@testing-library/jest-dom';
import { afterEach } from 'vitest';
import { cleanup } from '@testing-library/react';

// Ensure RTL cleanup between tests
afterEach(() => {
  cleanup();
});
