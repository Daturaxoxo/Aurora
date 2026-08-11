<p align="center">
  <img src="https://host.getaurora.moe/assets/web/embed.png" alt="ᅠ" />
</p>
<div align="center">
  
# Aurora Başlatıcısı
Anime oyunları için yapılan hafif bir mod platformu.
</div>
<p align="center">
  <img src="https://img.shields.io/github/v/release/Daturaxoxo/Aurora?include_prereleases&color=007ec6&v=1" alt="Release" />
  <img src="https://img.shields.io/github/downloads/Daturaxoxo/Aurora/total?color=f1ff2e&v=2" alt="GitHub Downloads" />
  <img src="https://img.shields.io/github/contributors/Daturaxoxo/Aurora?color=ff2ef1&v=3" alt="Contributors" />
  <object data="https://getaurora.moe" type="text/html">
    <a href="https://getaurora.moe">
      <img src="https://img.shields.io/badge/Official-Website-blue&color=FFFFFF?logo=cloudnativebuild&v=0" alt="Website Link" />
    </a>
  </object>
  <object data="https://virustotal.com" type="text/html">
    <a href="https://www.virustotal.com/">
      <img src="https://img.shields.io/badge/Antivirus-Scan-2EC7FF?logo=virustotal&logoColor=white" alt="VirusTotal Scan" />
    </a>
  </object>
</p>
<p align="center">
  <a href="https://github.com/Daturaxoxo/Aurora/blob/rewrite/README.md">English</a> | <a href="https://github.com/Daturaxoxo/Aurora/blob/rewrite/README.cn.md">中文</a> | <a href="https://github.com/unchihugo/FluentFlyout/blob/master/README.jp.md">日本語</a> | <strong>Türkçe</strong> | <a href="https://github.com/unchihugo/FluentFlyout/blob/master/README.es.md">Español</a>
</p>
<br></br>
Aurora, Unreal Engine tabanlı anime oyunları için yapılmış hafif bir mod platformudur. Oyunlarına kolayca Unreal Engine 5 PAK modları, Lua komutları ve blueprint'leri
serbestçe yüklemenize yardım eder. Temiz bir masaüstü arayüzü ile ve son derece kolay kurulumu ile favori oyunlarına mod eklemeye başla.
<br></br>

Hem `.pak` karakter modeli modlarını hem de `.asi` DLL modlarını oyununuzda kolayca yükleyin; daha fazla istiyorsan Lua komut dosyaları da desteklenmektedir.

Aurora, [Rust](https://rust-lang.org) ile yazılmış ve [Slint](https://slint.dev) ile işlenmiştir; her ikisi de son derece hafif dilleridir.
<br></br>

> [!NOTE]
> Uygulamamız imzalanmamış olduğundan, Windows Defender Aurora'yı ilk başlatıldığında yanlışlıkla tehlike olarak işaretleyebilir. Microsoft'un [Smart App Control](https://learn.microsoft.com/en-us/windows/apps/develop/smart-app-control/overview) sistemini desteklemiyoruz. Aurora'yı ilk kez çalıştırırken Smart App Control tarafından engellenme olasılığınız oldukça yüksektir.
>
> Smart App Control'ü nasıl devre dışı bırakacağınızı öğrenin: [Buraya tıklayın!](https://docs.getaurora.moe/hidden/guides/smart-app-control)
<br>
</br>

> [!IMPORTANT]
> Aurora, `.ini` modlarını desteklemez ve asla desteklemeyecektir; bu modlar, 3DMigoto veya XXMI gibi D3D11 hooking projeleri tarafından yüklenir. Bunlar için destek oluşturmak neredeyse imkansızdır; Aurora ile 3DMigoto arasındaki mimari büyük ölçüde farklıdır. Biz bir Unreal Engine PAK yükleyicisiyiz, bir DirectX hooking aracı değiliz.
<br>
</br>

<h2 align="left">
  <img src="https://img.icons8.com/?size=32&id=xfE5l4OXJWrc&format=png&color=000000" height="24" alt=""> Özellikler
</h2>

- **Kolay Kurulum** — Aurora, yeni başlayanlar için kullanıcı dostu olacak şekilde tasarlanmıştır; her şeyi kurmak için bir kurulum programı sunuyoruz ve yol bulma modülümüz oyun kurulumunuzu otomatik olarak bulur.
- **Tüm sürümler için destek** — Tüm sürümleri ve sağlayıcıları destekliyoruz:
- - **Sürümler** — Global, Tayvan, Çin
- - **Platformlar** — Yerel (Perfect World İstemcisi), Steam, Epic Games
- **Mod Yöneticisi** — Yeni başlayanlar için yeterince basit, ne yaptığını bilenler için yeterince gelişmiş:
- - **Etkinleştirme/Devre Dışı Bırakma** — Modlarınızı tek tıklamayla devre dışı bırakın ve etkinleştirin
- - **Yeniden Adlandırma** — Mod yöneticisi içinden modun disk üzerindeki adını değiştirin
- - **Silme** — Modu tek tıklamayla silin; yanlışlıkla silme durumlarını önlemek için bir uyarı penceresi gösterilir.
- - **Arama** — Yüklü modları isme göre arayın
- - **Filtreleme** — Modları etkin/devre dışı durumuna, yazara veya karaktere göre filtreleyin.
- - **Simgeler** — Yerleşik kütüphanemizden bir karakterin simgesini seçin veya özel bir resim kullanın.
- - **Görüntüleme Stilleri** — Kendi tarzınıza göre modların nasıl görüntüleneceğini belirlemek için liste görünümü veya ızgara görünümü arasında seçim yapın
- - **Mod Grupları** — Gruplar oluşturun ve modları bu gruplara sürükleyip bırakın; varsayılan olarak daraltılabilir.
- - **Toplu Seçim** — Birden fazla mod üzerinde aynı anda yeniden adlandırma, silme veya etkinleştirme/devre dışı bırakma gibi işlemleri gerçekleştirin.
- **Ekran Görüntüsü Yöneticisi** — Oyun içinde çektiğiniz ekran görüntülerini görüntüleyin
- **Lua Komut Dosyaları** — Başlatıcıdan Lua komut dosyaları oluşturun, düzenleyin ve bunları oyun içine yükleyin.
- **Eklenti Yöneticisi** — Varsayılan olarak yüklenmeyen, kullanışlı ve oyun deneyimini kolaylaştıran eklentileri doğrudan başlatıcıdan yükleyin.
- **Özel Motor** — Özel motorumuz **Everlight** ile oyununuz üzerinde tam kontrol sahibi olun. Herhangi bir çökmeyi önlemek için çok sayıda güvenlik kontrolü entegre edilmiştir.
<br>
</br>

<h2 align="left">
  <img src="https://img.icons8.com/?size=32&id=gXoJoyTtYXFg&format=png&color=ffffff" height="24" alt=""> Windows Kurulumu
</h2>

### Yükleyici
Aurora'yı kurmanın en kolay ve önerilen yolu.
1. Windows yükleyicisini [en son sürümden](https://github.com/Daturaxoxo/Aurora/releases/latest) indirin
2. Çalıştırın ve kurulumunuzu yapılandırın.
3. Kurulum tamamlandıktan sonra Aurora otomatik olarak başlayacaktır.

### Taşınabilir
1. Taşınabilir Windows paketini [en son sürümden](https://github.com/Daturaxoxo/Aurora/releases/latest) indirin
2. ZIP dosyasını Aurora'yı kurmak istediğiniz yere çıkarın.
3. Aurora.exe dosyasını çalıştırın
<br>
</br>
<h2 align="left">
  <img src="https://img.icons8.com/?size=100&id=17842&format=png&color=000000" height="24" alt=""> Linux Kurulumu
</h2>

> [!NOTE]
> Aurora'nın yerel bir Linux sürümü bulunsa da, oyunun gerçekten açıldığından emin olmak için oyunu DW-Proton ile çalıştırmanız gerekir. DW-Proton'un en son sürümleri yerine DW-Proton-10.0-26 kullanmanızı öneririz.
> 
> [ProtonPlus](https://github.com/Vysp3r/protonplus)'u indirmenizi öneriyoruz.

> 
> [!IMPORTANT]
> Aurora, root ve/veya sudo yetkileriyle düzgün çalışmaz. Oyun kurulumunuz root erişimi gerektiren bir klasördeyse, başka bir yere taşımanızı öneririz.

1. Linux Taşınabilir Sürümünü [en son sürümden](https://github.com/Daturaxoxo/Aurora/releases/latest) indirin.
2. ZIP dosyasını Aurora'yı kurmak istediğiniz yere çıkarın.
3. Aurora.AppImage dosyasını başlatın
<br>
</br>
<h2 align="left">
  <img src="https://img.icons8.com/?size=32&id=keI1M862UTP2&format=png&color=000000" height="24" alt=""> Kaynak Koddan Derleme
</h2>

> [!IMPORTANT]
> Uygulamayı kendiniz derlemek için şu araçlara ihtiyacınız var: [Rust Programlama Dili](https://rust-lang.org), [LLVM Derleyicisi](https://www.llvm.org)
1. Bu projenin ZIP kaynak kodunu indirin. (**Code Butonu > "Download ZIP"**)
2. Kaynak kodu istediğiniz konuma çıkarın.
3. Projenin kök klasöründe yönetici olarak bir komut istemi açın.
4. `cargo run` komutunu çalıştırın
<br>
</br>

<h2 align="left">
  <img src="https://img.icons8.com/?size=32&id=114474&format=png&color=000000" height="24" alt=""> Mod Yapımcıları için Araçlar
</h2>

Aurora, mod yapımcılarının kullanabileceği çeşitli araçlar sunar: hata ayıklama pencerelerinden dump kısayol tuşlarına kadar. Ayrıca modunuzun içine
koyabileceğiniz meta veri dosyalarını (`mod.json`) da destekler; böylece mod yöneticimiz modunuz hakkında daha fazla bilgi gösterebilir. Örnek olarak mod
sürümü, simge (dahili simgelerden seçebilir veya harici bir URL ile kendi özel simgenizi belirleyebilirsiniz), yazar ve destek bağlantısı verilebilir.
[Görüntüleme dosyası oluşturmayı](https://docs.getaurora.moe/mod-authors/displaying-your-mod) öğrenin
<br>
</br>

<h2 align="left">
  <img src="https://img.icons8.com/?size=32&id=31016&format=png&color=000000" height="24" alt=""> Aurora'yı Çevirmek
</h2>

Çeviri konusunda alabileceğimiz her türlü yardımı takdir ediyoruz. Uygulamamızın, web sitemizin veya hatta bu README dosyasının çevirilerine katkıda bulunmak
isterseniz, çevirmen belgelerimize göz atmaktan çekinmeyin:
[Çevirilerle Aurora'ya katkıda bulunmayı](https://docs.getaurora.moe/translations/translation-status) öğrenin
