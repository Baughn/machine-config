## INSTALLATION ##

The latest version of KanjiTomo can be downloaded from www.kanjitomo.net Unzip the package to 
any directory and double-click KanjiTomo.jar, launch.bat (Windows) or launch.sh (Linux/Mac) 
to run the program.  If you are upgrading from previous version of kanjitomo, you can use 
Settings -> Import settings to load your old configuration.

Java is required to run the program. It is recommended to install Java JDK instead of JRE, it 
will increase recognition speed by about 25%. KanjiTomo is a standalone program, Java browser 
plugin is not required and can be disabled. Java can be downloaded from 
http://www.oracle.com/technetwork/java/javase/downloads/index.html

KanjiTomo has been tested on Windows 7 operating system. Other operating systems might also 
work, but Japanese fonts must to be installed.


## DOCUMENTATION ##

See instructions.html for documentation


## LICENSE ##

See LICENSE.txt for details


## CONTACT ##

If you have any questions about the program, feel free to send an email to kanjitomo@gmail.com


## RELEASE HISTORY ##

1.0.4 (2020-08-18)
------------------
- target character color can be selected with a hotkey (see config.txt:SET_CHARACTER_COLOR),
  this can be used when automatic character detection fails with colored text or background
- fixed occasional crashes with automatic OCR mode
- fixed expand/contract match hotkeys

1.0.3 (2020-05-25)
------------------
- fixed font scaling with config.txt parameters

1.0.0 (2019-12-15)
------------------
- improved OCR quality and speed
- KanjiTomo's algorithm is now available as a Java library at GitHub
  https://github.com/sakarika/kanjitomo-ocr

0.9.13 (2018-11-25)
-------------------
- fixed issue that prevented launch with Java 11

0.9.12 (2016-01-24)
-------------------
- improved text orientation detection

0.9.11 (2015-04-12)
-------------------
- support for Chinese language

0.9.10 (2014-05-18)
---------------------
- ability to save identified words to a list and export them 
  to file or clipboard 

0.9.9 (2013-06-17)
------------------
- zoom mode
- ability to click and drag to manually select
  word location (in file or zoom mode)

0.9.8 (2013-02-10)
------------------
- dictionary for Japanese names
- improved detection of white text over complex background

0.9.7 (2012-11-25)
------------------
- clipboard support
- fullscreen mode
- two-page spread mode
- file history

0.9.5 (2012-10-07)
------------------
- four character compounds
- selectable text orientation
- selectable text color
- hotkeys
- faster startup speed

0.9.2 (2012-09-23) 
------------------
- initial release


## ACKNOWLEDGEMENTS ##

EDICT, ENAMDICT and KANJIDIC dictionaries are the property of the Electronic Dictionary Research and Development Group, and are used in conformance with 
the Group's licence. 
www.edrdg.org/jmdict/edict.html

CC-CEDICT dictionary files under Creative Commons Attribution-Share Alike 3.0 License http://creativecommons.org/licenses/by-sa/3.0/
http://cc-cedict.org/wiki/ 

imgscalr library by Riyad Kalla
https://github.com/rkalla/imgscalr

Unsharp Mask code by Romain Guy
http://www.java2s.com/Code/Java/Advanced-Graphics/UnsharpMaskDemo.htm

JKeyMaster library by Denis Tulskiy
https://github.com/tulskiy/jkeymaster

Kryo library by EsotericSoftware
https://github.com/EsotericSoftware/kryo
