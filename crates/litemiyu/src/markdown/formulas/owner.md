1. 一个"怪物积分"

$$\int_{-\infty}^{\infty}\!\int_{-\infty}^{\infty}\frac{e^{-(ax^2+2bxy+cy^2)}}{\sqrt{1+(x^2+y^2)^{3/2}}}\,dx\,dy=\frac{2\pi}{\sqrt{ac-b^2}}\sum_{k=0}^{\infty}\frac{(-1)^k}{k!}\left(\frac{a+c}{2(ac-b^2)}\right)^{k}\Gamma\!\left(\frac{k+1}{2}\right)$$

2. 连分数与嵌套根式

$$\cfrac{1}{1+\cfrac{1}{1+\cfrac{1}{1+\cfrac{1}{1+\ddots}}}}=\frac{\sqrt5-1}{2},\qquad \sqrt{1+2\sqrt{1+3\sqrt{1+4\sqrt{1+\cdots}}}}=3$$

3. Hilbert 矩阵的行列式

$$H_n=\left(\frac{1}{i+j-1}\right)_{1\le i,j\le n},\qquad \det H_n=\frac{\left(\prod_{k=1}^{n-1}k!\right)^{4}}{\prod_{k=1}^{2n-1}k!}$$

$$\begin{vmatrix} 1 & \frac12 & \frac13 & \cdots & \frac1n\\[2pt] \frac12 & \frac13 & \frac14 & & \vdots\\[2pt] \frac13 & \frac14 & \ddots & & \vdots\\[2pt] \vdots & & & \ddots & \frac{1}{2n-2}\\[2pt] \frac1n & \cdots & \cdots & \frac{1}{2n-2} & \frac{1}{2n-1} \end{vmatrix} =\frac{{(n-1)!(n-2)!\cdots 1!}^{4}}{(2n-1)!(2n-2)!\cdots 1!}$$

4. 广义相对论

$$\Gamma^{\mu}_{\nu\lambda}=\frac{1}{2} g^{\mu\sigma}\left(\partial_\nu g_{\sigma\lambda}+\partial_\lambda g_{\sigma\nu}-\partial_\sigma g_{\nu\lambda}\right)$$

$$R^{\rho}{}_{\sigma\mu\nu}=\partial_\mu\Gamma^{\rho}_{\nu\sigma}-\partial_\nu\Gamma^{\rho}_{\mu\sigma}+\Gamma^{\rho}_{\mu\lambda}\Gamma^{\lambda}_{\nu\sigma}-\Gamma^{\rho}_{\nu\lambda}\Gamma^{\lambda}_{\mu\sigma}$$

$$R_{\mu\nu}-\frac{1}{2} R\,g_{\mu\nu}+\Lambda g_{\mu\nu}=\frac{8\pi G}{c^4}T_{\mu\nu},\qquad T_{\mu\nu}=\left(\rho+\frac{p}{c^2}\right)u_\mu u_\nu+p\,g_{\mu\nu}$$

5. 路径积分 + Euler–Lagrange

$$\langle q_f,t_f\mid q_i,t_i\rangle=\int\mathcal{D}q(t)\,\exp\!\left[\frac{i}{\hbar}\int_{t_i}^{t_f} dt\left(\frac{m}{2}\dot q^{2}-V(q)\right)\right]$$

$$\frac{\partial L}{\partial q^a}-\frac{d}{dt}\frac{\partial L}{\partial\dot q^a}=0\;\Longleftrightarrow\;\delta\!\int_{t_1}^{t_2} L(q,\dot q,t)dt=0$$

6. 标准模型拉氏量（压缩版）

$$\mathcal{L}_{\mathrm{SM}}=-\frac{1}{4} F^{a}_{\mu\nu}F^{a\mu\nu}+i\bar\psi\,\gamma^{\mu}D_\mu\psi+|D_\mu\Phi|^{2}-\mu^{2}\Phi^{\dagger}\Phi-\lambda(\Phi^{\dagger}\Phi)^{2}-\bar\psi_i\,y_{ij}\,\psi_j\Phi+\text{h.c.}$$

7. 统计力学配分函数（玻色/费米 + 巨正则）

$$Z_{\mathrm{gc}}=\prod_{k}\left[1\mp e^{-\beta(\varepsilon_k-\mu)}\right]^{\mp1},\qquad \langle n_k\rangle=\frac{1}{e^{\beta(\varepsilon_k-\mu)}\mp1}$$

$$Z_N=\frac{1}{N!\,h^{3N}}\int\!\cdots\!\int e^{-\beta H(q,p)}\,d^{3N}q\,d^{3N}p\,=\frac{1}{N!}\left(\frac{V}{\lambda_T^{3}}\right)^{N},\qquad \lambda_T=\frac{h}{\sqrt{2\pi m\,k_B\,T}}$$

8. 数论：Riemann ζ 的函数方程

$$\zeta(s)=2^{s}\pi^{s-1}\sin\!\left(\frac{\pi s}{2}\right)\Gamma(1-s)\,\zeta(1-s)$$

$$\xi(s)=\frac{1}{2} s(s-1)\pi^{-s/2}\Gamma\!\left(\frac{s}{2}\right)\zeta(s)=\xi(1-s)$$

9. Ramanujan 的 1/π

$$\frac{1}{\pi}=\frac{2\sqrt{2}}{9801}\sum_{k=0}^{\infty}\frac{(4k)!\,(1103+26390k)}{(k!)^{4}\,396^{4k}}$$

10. 贝叶斯 + 变分推断（密集恐惧款）

$$\underbrace{p(\theta\mid D)}_{\text{后验}}=\frac{\overbrace{p(D\mid\theta)}^{\text{似然}}\;\overbrace{p(\theta)}^{\text{先验}}}{\underbrace{\int_{\Theta}p(D\mid\vartheta)\,p(\vartheta)\,d\vartheta}_{\text{边缘似然}}}$$

$$\log p(D)=\underbrace{\mathbb{E}_{q}\!\left[\log\frac{q(\theta)}{p(D,\theta)}\right]}_{-\mathrm{KL}\,(q\,\|\,p)}\;+\;\underbrace{\mathbb{E}_{q}\!\left[\log\frac{p(D,\theta)}{q(\theta)}\right]}_{\mathcal{L}(q)\,\le\,\log p(D)}$$

11. 一个多行对齐推导

$$\begin{aligned} I(a)&=\int_{0}^{\infty}\frac{dx}{(1+x^{2})(1+x^{a})} =\int_{0}^{1}\frac{dx}{(1+x^{2})(1+x^{a})}+\int_{1}^{\infty}\frac{dx}{(1+x^{2})(1+x^{a})}\\[4pt] &=\int_{0}^{1}\frac{dx}{(1+x^{2})(1+x^{a})}+\int_{0}^{1}\frac{x^{a}\,dx}{(1+x^{2})(1+x^{a})}\\[4pt] &=\int_{0}^{1}\frac{dx}{1+x^{2}}=\left.\arctan x\right|_{0}^{1}=\frac{\pi}{4}\qquad(\forall\,a\in\mathbb{R}) \end{aligned}$$
