import { defineConfig } from 'vitest/config';

export default defineConfig({
  test: {
    include: ['test/**/*.spec.ts'],
    environment: 'node',
    testTimeout: 5000,
    hookTimeout: 5000,
    fileParallel: false,
  },
});
