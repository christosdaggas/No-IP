Name:           noip-duc
Version:        3.3.0
Release:        1%{?dist}
Summary:        No-IP Dynamic DNS Update Client
License:        Apache-2.0
URL:            https://www.noip.com
Source0:        noip-duc-%{version}.tar.gz

# We skip the normal build phase; binaries are pre-built with cargo
AutoReqProv:    yes

%description
No-IP Dynamic Update Client (DUC) keeps your dynamic IP address
updated with No-IP's DNS servers. Includes both a CLI daemon and
a graphical interface.

%install
rm -rf %{buildroot}
install -d %{buildroot}%{_bindir}
install -m 0755 %{_sourcedir}/noip-duc %{buildroot}%{_bindir}/noip-duc
install -m 0755 %{_sourcedir}/noip-duc-gui %{buildroot}%{_bindir}/noip-duc-gui

install -d %{buildroot}%{_unitdir}
install -m 0644 %{_sourcedir}/noip-duc.service %{buildroot}%{_unitdir}/noip-duc.service

install -d %{buildroot}%{_datadir}/applications
install -m 0644 %{_sourcedir}/com.noip.DUC.desktop %{buildroot}%{_datadir}/applications/com.noip.DUC.desktop

install -d %{buildroot}%{_metainfodir}
install -m 0644 %{_sourcedir}/com.noip.DUC.metainfo.xml %{buildroot}%{_metainfodir}/com.noip.DUC.metainfo.xml

# Icons
for sz in 16 32 48 64 128 256 512; do
    install -d %{buildroot}%{_datadir}/icons/hicolor/${sz}x${sz}/apps
    install -m 0644 %{_sourcedir}/icons/${sz}.png %{buildroot}%{_datadir}/icons/hicolor/${sz}x${sz}/apps/com.noip.DUC.png
done
install -d %{buildroot}%{_datadir}/icons/hicolor/scalable/apps
install -m 0644 %{_sourcedir}/icons/com.noip.DUC.svg %{buildroot}%{_datadir}/icons/hicolor/scalable/apps/com.noip.DUC.svg

install -d %{buildroot}%{_docdir}/%{name}
install -m 0644 %{_sourcedir}/LICENSE %{buildroot}%{_docdir}/%{name}/LICENSE
install -m 0644 %{_sourcedir}/README.md %{buildroot}%{_docdir}/%{name}/README.md

%files
%{_bindir}/noip-duc
%{_bindir}/noip-duc-gui
%{_unitdir}/noip-duc.service
%{_datadir}/applications/com.noip.DUC.desktop
%{_metainfodir}/com.noip.DUC.metainfo.xml
%{_datadir}/icons/hicolor/16x16/apps/com.noip.DUC.png
%{_datadir}/icons/hicolor/32x32/apps/com.noip.DUC.png
%{_datadir}/icons/hicolor/48x48/apps/com.noip.DUC.png
%{_datadir}/icons/hicolor/64x64/apps/com.noip.DUC.png
%{_datadir}/icons/hicolor/128x128/apps/com.noip.DUC.png
%{_datadir}/icons/hicolor/256x256/apps/com.noip.DUC.png
%{_datadir}/icons/hicolor/512x512/apps/com.noip.DUC.png
%{_datadir}/icons/hicolor/scalable/apps/com.noip.DUC.svg
%doc %{_docdir}/%{name}/LICENSE
%doc %{_docdir}/%{name}/README.md

%post
%systemd_post noip-duc.service

%preun
%systemd_preun noip-duc.service

%postun
%systemd_postun_with_restart noip-duc.service

%changelog
* Mon Mar 10 2026 No-IP Team <support@noip.com> - 3.3.0-1
- Initial RPM package
- CLI daemon and GUI client
- Systemd service integration
