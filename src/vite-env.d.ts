/// <reference types="vite/client" />

declare module 'react-katex' {
  import { ComponentType } from 'react';
  
  interface MathProps {
    children: string;
    errorColor?: string;
    renderError?: (error: any) => React.ReactNode;
  }
  
  export const InlineMath: ComponentType<MathProps>;
  export const BlockMath: ComponentType<MathProps>;
}
