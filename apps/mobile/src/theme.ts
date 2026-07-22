import { colors, radii, spacing } from '@yorm/design-tokens';

export const theme = {
  colors: {
    background: colors.paper,
    surface: '#FFFFFF',
    muted: colors.stone,
    accent: colors.coral,
    text: colors.black,
    subduedText: '#5E5A54',
    error: '#9F261D',
  },
  radii,
  spacing,
} as const;
