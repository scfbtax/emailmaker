# emailmaker

This is a simple email macro that takes in two files, the first is an excel file and the second is the template of the email written in Rust.

The example template can be found here [template.txt](https://github.com/user-attachments/files/18989296/template.txt). As you can see the replacement objects should be wrapped in {} format, these objects are populated from the header of the spreadsheet an example of which can be found here [test.xlsx](https://github.com/user-attachments/files/18989407/test.xlsx). Always name the email address column "email address" case insensitive. 

Name Logic:
The program currently recognizes all columns that have the word name on them. It 
is advised to keep the formatting to the one in the example. Name, for the first name, Last Name for the last name in the case you have them separated. If they are all in one column, then proceed with the column title "name".

If there are any errors or bugs please reach out to me. The program can be downloaded by going to release and then downloading the emailmaker.exe
