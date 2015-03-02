(custom-set-variables
 ;; custom-set-variables was added by Custom.
 ;; If you edit it by hand, you could mess it up, so be careful.
 ;; Your init file should contain only one such instance.
 ;; If there is more than one, they won't work right.
 '(browse-url-browser-function (quote browse-url-chromium))
 '(browse-url-chromium-program "google-chrome-stable")
 '(column-number-mode t)
 '(elisp-cache-byte-compile-files t)
 '(elisp-cache-freshness-delay 1440)
 '(font-lock-maximum-size 256000)
 '(global-undo-tree-mode t)
 '(haskell-font-lock-symbols (quote unicode))
 '(haskell-mode-hook
   (quote
    (imenu-add-menubar-index turn-on-eldoc-mode turn-on-haskell-decl-scan turn-on-haskell-doc turn-on-haskell-indentation)))
 '(highlight-changes-face-list
   (quote
    (highlight-changes-1 highlight-changes-2 highlight-changes-3 highlight-changes-4 highlight-changes-5 highlight-changes-6 highlight-changes-7)))
 '(highlight-changes-global-changes-existing-buffers t)
 '(highlight-changes-invisible-string " -Chg")
 '(highlight-changes-visible-string " +Chg")
 '(ido-auto-merge-delay-time 0.7)
 '(ido-auto-merge-inhibit-characters-regexp "[][*?~]")
 '(ido-auto-merge-work-directories-length 0)
 '(ido-cache-ftp-work-directory-time 1.0)
 '(ido-cache-unc-host-shares-time 8.0)
 '(ido-cannot-complete-command (quote ido-completion-help))
 '(ido-completion-buffer "*Ido Completions*")
 '(ido-default-buffer-method (quote selected-window))
 '(ido-enable-tramp-completion nil)
 '(ido-everywhere t)
 '(ido-max-dir-file-cache 100)
 '(ido-max-directory-size 300000)
 '(ido-max-file-prompt-width 0.35)
 '(ido-max-prospects 12)
 '(ido-max-work-directory-list 50)
 '(ido-max-work-file-list 10)
 '(ido-record-ftp-work-directories nil)
 '(ido-save-directory-list-file "~/.ido.last")
 '(indent-tabs-mode nil)
 '(jit-lock-chunk-size 10000)
 '(jit-lock-context-time 0.5)
 '(jit-lock-stealth-load 800)
 '(jit-lock-stealth-nice 0.1)
 '(jit-lock-stealth-time 0.5)
 '(jit-lock-stealth-verbose nil)
 '(js2-auto-indent-p t)
 '(js2-basic-offset 4)
 '(js2-dynamic-idle-timer-adjust 0)
 '(js2-enter-indents-newline t)
 '(js2-highlight-level 3)
 '(js2-idle-timer-delay 0.2)
 '(js2-indent-on-enter-key t)
 '(js2-language-version 200)
 '(js2-mirror-mode nil)
 '(js2-pretty-multiline-declarations t)
 '(js3-auto-indent-p t)
 '(js3-enter-indents-newline t)
 '(pabbrev-global-mode-buffer-size-limit nil)
 '(pabbrev-idle-timer-verbose nil)
 '(pabbrev-marker-distance-before-scavenge 2000)
 '(pabbrev-scavenge-some-chunk-size 80)
 '(pabbrev-thing-at-point-constituent (quote symbol))
 '(pdb-path (quote /usr/lib/python2\.7/pdb\.py))
 '(py-backspace-function (quote backward-delete-char-untabify))
 '(py-continuation-offset 4)
 '(py-current-defun-delay 2)
 '(py-delete-function (quote delete-char))
 '(py-encoding-string " # -*- coding: utf-8 -*-")
 '(py-extensions "py-extensions.el")
 '(py-import-check-point-max 20000)
 '(py-indent-offset 2 t)
 '(py-ipython-history "~/.ipython/history")
 '(py-lhs-inbound-indent 1)
 '(py-master-file nil)
 '(py-outline-mode-keywords
   (quote
    ("class" "def" "elif" "else" "except" "for" "if" "while" "finally" "try" "with")))
 '(py-pdbtrack-minor-mode-string " PDB")
 '(py-pep8-command "pep8")
 '(py-pychecker-command "pychecker")
 '(py-pyflakes-command "pyflakes")
 '(py-pyflakespep8-command "pyflakespep8.py")
 '(py-pylint-command "pylint")
 '(py-python-history "~/.python_history")
 '(py-rhs-inbound-indent 1)
 '(py-send-receive-delay 5)
 '(py-separator-char 47)
 '(py-shebang-startstring "#! /bin/env")
 '(py-shell-input-prompt-1-regexp "^>>> ")
 '(python-mode-modeline-display "Py")
 '(save-place t nil (saveplace))
 '(show-paren-mode t)
 '(sort-fold-case t t)
 '(standard-indent 4)
 '(tab-width 2)
 '(tool-bar-mode nil)
 '(tramp-default-method "ssh")
 '(undo-tree-visualizer-diff t)
 '(uniquify-buffer-name-style (quote post-forward) nil (uniquify))
 '(whitespace-empty (quote whitespace-empty))
 '(whitespace-empty-at-bob-regexp "\\`\\(\\([   ]*
\\)+\\)")
 '(whitespace-empty-at-eob-regexp "^\\([
]+\\)\\'")
 '(whitespace-hspace (quote whitespace-hspace))
 '(whitespace-hspace-regexp "\\(\\(\240\\|ࢠ\\|ठ\\|ภ\\|༠\\)+\\)")
 '(whitespace-indentation (quote whitespace-indentation))
 '(whitespace-line (quote whitespace-line))
 '(whitespace-line-column 80)
 '(whitespace-newline (quote whitespace-newline))
 '(whitespace-space (quote whitespace-space))
 '(whitespace-space-after-tab (quote whitespace-space-after-tab))
 '(whitespace-space-before-tab (quote whitespace-space-before-tab))
 '(whitespace-space-before-tab-regexp "\\( +\\)\\(      +\\)")
 '(whitespace-space-regexp "\\( +\\)")
 '(whitespace-style (quote (tabs trailing)))
 '(whitespace-tab (quote whitespace-tab))
 '(whitespace-tab-regexp "\\(   +\\)")
 '(whitespace-trailing (quote whitespace-trailing)))
(custom-set-faces
 ;; custom-set-faces was added by Custom.
 ;; If you edit it by hand, you could mess it up, so be careful.
 ;; Your init file should contain only one such instance.
 ;; If there is more than one, they won't work right.
 '(highlight-changes ((((min-colors 88) (class color)) (:weight thin))))
 '(highlight-changes-delete ((((min-colors 88) (class color)) (:strike-through t)))))

;; ELPA
(require 'package)
(setq package-archives
      '(("gnu" . "http://elpa.gnu.org/packages/")
        ("marmalade" . "http://marmalade-repo.org/packages/")
        ("melpa" . "http://melpa.milkbox.net/packages/")))
(defvar desired-packages
  '(indent-guide column-marker nyan-mode smex pov-mode ipython ein js2-mode js3-mode
                 multiple-cursors flyspell-lazy))

;; Google
(let ((path "/usr/local/google/home/svein/.emacs-google"))
  (and (file-exists-p path)
       (load path)
       (push '("GELPA" . "http://internal-elpa.appspot.com/packages/")
             package-archives)
       (push 'borgsearch desired-packages)
       (push 'citc desired-packages)
       (push 'ditrack-procfs desired-packages)
       (push 'fast-file-attributes desired-packages)
       (push 'squery desired-packages)))

(package-initialize)

;; Install any missing packages
(unless package-archive-contents
  (package-refresh-contents))
(dolist (package desired-packages)
  (unless (package-installed-p package)
    (package-install package)))

;; Misc. setup
(server-start)
(require 'uniquify)
(blink-cursor-mode (- (*) (*) (*)))
(global-set-key "\M- " 'hippie-expand)

;; Spellchecking
;(flyspell-lazy-mode)
(dolist (hook '(python-mode-hook c-mode-hook c++-mode-hook borg-mode-hook hook
                                 javascript-mode-hook js-mode-hook js2-mode-hook
                                 lisp-mode-hook emacs-lisp-mode-hook))
  (add-hook hook (lambda () (flyspell-prog-mode))))

;; Put backup files in /tmp
(setq backup-directory-alist
      `((".*" . ,temporary-file-directory)))
(setq auto-save-file-name-transforms
      `((".*" ,temporary-file-directory t)))

;; Haskell
;(push "/usr/local/google/home/svein/src/haskellmode-emacs/" load-path)
;(load "haskell-site-file")
;(add-hook 'haskell-mode-hook 'turn-on-haskell-doc-mode)
;(add-hook 'haskell-mode-hook 'turn-on-haskell-indentation)


(defun byte-compile-dest-file (fn)
  (concat fn "c"))

;; Misc. bindings and setup
(global-set-key [s-backspace] 'kill-buffer)
(global-set-key (kbd "M-s-b") 'switch-to-buffer)
(global-set-key (kbd "RET") 'newline-and-indent)
(global-set-key (kbd "s-o") 'other-window)
(global-set-key (kbd "M-g") 'goto-line)
(global-set-key (kbd "M-s") 'isearch-repeat-forward)
(global-set-key [f6] 'gsearch)
(global-set-key [f7] 'google-show-tag-locations-regexp)
(global-set-key [f8] 'google-show-callers)
(global-set-key [f9] 'google-pop-tag)
(global-set-key [f10] 'google-show-matching-tags)
(global-set-key (kbd "M-r") 'revert-buffer)
(global-set-key (kbd "C-x M-e") 'g4-edit-open-asynchronously)
(global-set-key (kbd "M-o") 'other-window)
(global-hi-lock-mode 1)
(global-set-key (kbd "M-i") 'indent-rigidly)
(global-set-key (kbd "C-x M-s") 'sort-lines)
(global-set-key (kbd "s-s") 'save-buffer)
(global-set-key (kbd "C-c C-g") 'autogen)
(global-set-key (kbd "<C-S-up>")     'buf-move-up)
(global-set-key (kbd "<C-S-down>")   'buf-move-down)
(global-set-key (kbd "<C-S-left>")   'buf-move-left)
(global-set-key (kbd "<C-S-right>")  'buf-move-right)
;; (global-highlight-changes-mode 1)
(show-paren-mode 1)

;; Identation~~
(require 'indent-guide)
(indent-guide-global-mode)

;; Open path under cursor, etc.
(defvar buffer-stack nil)
(defun push-file-and-open (path)
  (push (current-buffer) buffer-stack)
  (find-file path))
(defun pop-file ()
  (interactive)
  (let ((old (pop buffer-stack)))
    (when old
      (switch-to-buffer old))))
(global-set-key
 (kbd "C-.")
 (lambda ()
   (interactive)
   (let ((path (if (region-active-p)
                   (buffer-substring-no-properties (region-beginning) (region-end))
                 (thing-at-point 'filename))))
     (if (string-match-p "\\`https?://" path)
         (browse-url path)
       (progn ; not starting “http://”
         (if (file-exists-p path)
             (push-file-and-open path)
           (if (file-exists-p (concat path ".el"))
               (push-file-and-open (concat path ".el"))
             (when (y-or-n-p (format "file doesn't exist: %s. Create?" path))
               (push-file-and-open path)))))))))
(global-set-key (kbd "C-,") 'pop-file)
                              

;; 80-column markers.
(require 'column-marker)
(defun setup-column-marker () 
  (interactive)
  ;; This errors out on read-only files. TODO: Debug.
  (condition-case ex
    (column-marker-1 80)
    ('error (progn))))
(add-hook 'python-mode-hook 'setup-column-marker)
(add-hook 'c-mode-hook 'setup-column-marker)
(add-hook 'c++-mode-hook 'setup-column-marker)
(add-hook 'borg-mode-hook 'setup-column-marker)
(add-hook 'java-mode-hook 'setup-column-marker)
;; (add-hook 'go-mode-hook 'setup-column-marker)
(add-hook 'javascript-mode-hook 'setup-column-marker)
(add-hook 'js-mode-hook 'setup-column-marker)
(add-hook 'js2-mode-hook 'setup-column-marker)
;; (add-hook 'haskell-mode-hook 'setup-column-marker)
(color-theme-initialize)
(color-theme-subtle-hacker)
(setq inhibit-startup-message t)

;; Multiple cursors!
(global-set-key (kbd "C-c <") 'mc/mark-all-dwim)


;; Buffer switching
(defun switch-to-previous-buffer ()
  (interactive)
  (switch-to-buffer (other-buffer)))
(global-set-key [f1] 'switch-to-previous-buffer)

;; Recursive edit
(global-set-key (kbd "s-[") (lambda () (interactive) (save-window-excursion (save-excursion (recursive-edit)))))
(global-set-key (kbd "s-]") 'exit-recursive-edit)
(global-set-key (kbd "s-M-)") 'abort-recursive-edit)

;; Ido magic
(require 'ido)
(add-to-list 'ido-work-directory-list-ignore-regexps tramp-file-name-regexp)
(ido-mode t)
(setq ido-enable-flex-matching t) ;; enable fuzzy matching
(setq ido-execute-command-cache nil)
(global-set-key (kbd "M-b") 'ido-switch-buffer)
(global-set-key (kbd "M-x") 'smex)


;; Nyanmacs!
(require 'nyan-mode)
(nyan-mode 1)

;; Org-mode
(require 'org)
(define-key global-map "\C-cl" 'org-store-link)
(define-key global-map "\C-ca" 'org-agenda)
(setq org-log-done t)
(setq org-agenda-files (list "~/org/tasks.org"))

;; Js3-mode
;; (push "/usr/local/google/home/svein/.emacsd/js2-mode" load-path)
;; (autoload 'js2-mode "js2-mode" nil t)
(add-to-list 'auto-mode-alist '("\\.js$" . js3-mode))
;; ;;; adds symbols included by google closure to js2-additional-externs
;; (add-hook 'js2-post-parse-callbacks
;;   (lambda ()
;;     (let ((buf (buffer-string))
;;           (index 0))
;;       (while (string-match "\\(goog\\.require\\|goog\\.provide\\)('\\([^'.]*\\)" buf index)
;;         (setq index (+ 1 (match-end 0)))
;;         (add-to-list 'js2-additional-externs (match-string 2 buf))))))
;; (add-hook 'js2-mode-hook
;;   (lambda ()
;;     (local-set-key (kbd "s-y") 'js2-mode-toggle-element)))
