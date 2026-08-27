# THIS FILE IS AUTO-GENERATED. DO NOT MODIFY!!

# Copyright 2020-2023 Tauri Programme within The Commons Conservancy
# SPDX-License-Identifier: Apache-2.0
# SPDX-License-Identifier: MIT

-keep class io.github.rivulet.rivulet.* {
  native <methods>;
}

-keep class io.github.rivulet.rivulet.WryActivity {
  public <init>(...);

  void setWebView(io.github.rivulet.rivulet.RustWebView);
  java.lang.Class getAppClass(...);
  int getId();
  java.lang.String getVersion();
  int startActivity(...);
}

-keep class io.github.rivulet.rivulet.Ipc {
  public <init>(...);

  @android.webkit.JavascriptInterface public <methods>;
}

-keep class io.github.rivulet.rivulet.RustWebView {
  public <init>(...);

  void loadUrlMainThread(...);
  void loadHTMLMainThread(...);
  void evalScript(...);
}

-keep class io.github.rivulet.rivulet.RustWebChromeClient,io.github.rivulet.rivulet.RustWebViewClient {
  public <init>(...);
}
