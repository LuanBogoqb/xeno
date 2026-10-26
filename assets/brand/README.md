# XENO — pacote de identidade

PNGs derivados da referência aprovada, usando a ferramenta integrada image_gen. As duas versões brancas transparentes foram convertidas diretamente dos PNGs pretos transparentes, com autorização do usuário, preservando o canal alfa e alterando apenas RGB para branco.

- symbol-* : símbolo isolado.
- lockup-* : símbolo com XENO embaixo.
- banner-* : composição horizontal para o repositório.
- black-on-white : arte preta sobre fundo branco.
- white-on-black : arte branca sobre fundo preto.
- black-transparent / white-transparent : arte preta / branca com canal alfa.

As versões transparentes brancas podem parecer vazias em um visualizador de fundo branco; use sobre fundo escuro.

A imagem aprovada original está em lockup-black-on-white.png. As demais são derivações geradas e podem apresentar pequenas diferenças; estes arquivos são raster PNG, não vetores.

## Prompts usados

As tentativas de gerar os transparentes brancos tiveram artefatos e foram descartadas. Os arquivos finais brancos transparentes são conversões diretas de cor dos respectivos pretos transparentes; não são novas gerações.

Base:

Use case: logo-brand. Production asset export of the APPROVED XENO identity in reference, not a redesign. Preserve exact circular X silhouette, straight corrected upper-right inner junction, and wordmark geometry including all three E bars flush on right. Flat pure solid colors, crisp edges, no texture, no glow, no shadows, no mockups. 

### symbol-black-transparent

transparent_background=true

Square canvas. ONLY the circular X emblem, remove all XENO lettering. Solid pure BLACK emblem on genuinely TRANSPARENT background, including inner negative space. Center emblem with modest equal padding.

### symbol-white-transparent

transparent_background=true

Square canvas. ONLY the circular X emblem, remove all XENO lettering. Solid pure WHITE emblem on genuinely TRANSPARENT background, including inner negative space. Center emblem with modest equal padding.

### symbol-black-on-white

transparent_background=false

Square canvas. ONLY circular X emblem, remove all lettering. Pure BLACK emblem centered on pure WHITE background. Modest equal padding.

### symbol-white-on-black

transparent_background=false

Square canvas. ONLY circular X emblem, remove all lettering. Pure WHITE emblem centered on pure BLACK background. Modest equal padding.

### lockup-black-transparent

transparent_background=true

Preserve stacked emblem over XENO text exactly as reference. Pure BLACK artwork on genuinely TRANSPARENT background, including all negative space. Tightly fitted canvas with balanced modest padding.

### lockup-white-transparent

transparent_background=true

Preserve stacked emblem over XENO text exactly as reference. Pure WHITE artwork on genuinely TRANSPARENT background, including all negative space. Tightly fitted canvas with balanced modest padding.

### lockup-white-on-black

transparent_background=false

Preserve stacked emblem over XENO text exactly as reference. Pure WHITE artwork on pure BLACK background. Balanced padding.

### banner-light

transparent_background=false

Wide horizontal repository banner aspect ratio 3:1. Pure white background, pure black emblem on LEFT with XENO wordmark to its RIGHT, centered as a horizontal lockup. Use EXACT original emblem and EXACT lettering shapes from reference, simply arrange side by side. Generous safe margins, minimal and professional. No additional text.

### banner-dark

transparent_background=false

Wide horizontal repository banner aspect ratio 3:1. Pure black background, pure white emblem on LEFT with XENO wordmark to its RIGHT, centered as a horizontal lockup. Use EXACT original emblem and EXACT lettering shapes from reference, simply arrange side by side. Generous safe margins, minimal and professional. No additional text.

